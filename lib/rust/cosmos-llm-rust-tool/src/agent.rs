//! A multi-turn tool-calling loop that drives a model through a toolset.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use cosmos_llm::{Client, CompletionRequest, CompletionResponse, CosmosError, Message, ToolCall};
use serde_json::Value;

use crate::{Executor, Registry, Session};

/// Anything that can answer a completion request.
///
/// [`Client`] implements this, and so can a test double or a caller's own
/// wrapper. The loop takes this rather than a concrete client so a run can be
/// tested without a network, and so a caller who has already built their own
/// caching or logging layer can drive the loop through it.
pub trait Completer: Send + Sync {
    /// Sends one request and returns the provider's response.
    fn completion<'a>(
        &'a self,
        req: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse, CosmosError>> + Send + 'a>>;
}

impl Completer for Client {
    fn completion<'a>(
        &'a self,
        req: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse, CosmosError>> + Send + 'a>> {
        Box::pin(async move { Client::completion(self, req.clone()).await })
    }
}

/// Why a run ended.
///
/// Reported as itself rather than as a generic stop, because "the model
/// decided it was finished" and "it hit the step cap" mean very different
/// things when reading a run afterwards, and a bare step count cannot tell
/// them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// The model stopped calling tools, or a tool called [`Session::finish`].
    Finished,
    /// `max_steps` was reached with the model still working.
    Steps,
    /// The budget callback stopped the run mid-flight.
    Budget,
}

/// Usage accumulated across a run.
///
/// Providers vary in what they report and some report nothing at all, so these
/// are best-effort totals: a provider that returns no usage block leaves them
/// at zero rather than failing the run.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TokenUsage {
    /// Tokens consumed by prompts across every turn.
    pub prompt_tokens: u64,
    /// Tokens generated across every turn.
    pub completion_tokens: u64,
    /// Prompt plus completion, as reported.
    pub total_tokens: u64,
    /// Model turns taken so far.
    pub steps: u32,
    /// Dollars charged across every turn that reported a price.
    ///
    /// `None` until some turn reports one — see [`cosmos_llm::Usage::cost`],
    /// which only OpenRouter populates. A run mixing providers sums the turns
    /// that reported and silently omits the ones that did not, so a `Some`
    /// here is a floor on what the run cost, not a total. Budget on
    /// [`TokenUsage::total_tokens`] instead when the provider reports no price.
    pub cost: Option<f64>,
}

impl TokenUsage {
    /// Returns the accumulated cost, treating "not reported" as zero.
    ///
    /// Convenient for a budget check that would otherwise unwrap. Only use it
    /// where an unpriced run should read as free — against a provider that
    /// reports nothing, this reads zero forever and the ceiling never trips.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # #[cfg(feature = "agent")]
    /// # {
    /// use cosmos_llm_tool::TokenUsage;
    ///
    /// assert_eq!(TokenUsage::default().cost_or_zero(), 0.0);
    /// # }
    /// ```
    pub fn cost_or_zero(&self) -> f64 {
        self.cost.unwrap_or(0.0)
    }
}

/// What a finished run produced.
#[derive(Debug, Clone)]
pub struct RunOutcome {
    /// The model's last non-empty text output.
    pub text: String,
    /// How many model turns were taken.
    pub steps_taken: u32,
    /// Why the run ended.
    pub reason: StopReason,
    /// Accumulated token usage.
    pub usage: TokenUsage,
    /// The full message history, for inspection or replay.
    pub messages: Vec<Message>,
}

/// An event emitted as a run proceeds.
#[derive(Debug, Clone)]
pub enum LoopEvent<'a> {
    /// The model produced final text and stopped calling tools.
    Text(&'a str),
    /// A tool was dispatched.
    Tool {
        /// The tool's name.
        name: &'a str,
        /// The arguments the model supplied.
        input: &'a Value,
        /// What the tool returned, as the model will see it.
        result: &'a str,
    },
}

/// Drives a model through a toolset until it stops calling tools.
///
/// Deliberately small. The interesting decisions live elsewhere — the tools
/// enforce the invariants, a [`Session`] holds the caps, this only turns the
/// crank and counts what it costs.
///
/// # Provider neutrality
///
/// The loop speaks [`cosmos_llm::ToolCall`], which the client crate has
/// already normalized across providers: OpenAI encodes tool arguments as a
/// JSON string and Anthropic as a native object, and both arrive here as a
/// parsed [`Value`]. Replaying a turn goes through
/// [`CompletionResponse::to_assistant_message`] and [`Message::tool_result`],
/// which each provider maps to its own wire shape. Nothing in this file is
/// dialect-specific.
///
/// # A failing tool does not fail the run
///
/// A tool that errors has its error turned into the string the model sees as
/// that call's result. A raising tool would otherwise kill a run that is
/// mostly succeeding, throwing away every finding the agent had gathered — and
/// "your argument was invalid" is something a model can act on.
///
/// # Examples
///
/// ```rust
/// # use cosmos_llm_tool::{AgentLoop, Registry, ToolDefinition, ParameterType};
/// # use serde_json::json;
/// let mut registry = Registry::new();
/// registry.register(
///     ToolDefinition::new("echo")
///         .param("value", ParameterType::String, true, "text to echo")
///         .handler(|p| Ok(json!(p["value"].as_str().unwrap_or_default()))),
/// );
///
/// let agent = AgentLoop::new(registry).with_max_steps(5);
/// assert_eq!(agent.max_steps(), 5);
/// ```
pub struct AgentLoop {
    registry: Registry,
    model: Option<String>,
    system: Option<String>,
    max_steps: u32,
    schema: SchemaDialect,
    session: Option<Arc<Session>>,
    #[allow(clippy::type_complexity)]
    budget: Option<Box<dyn Fn(&TokenUsage) -> bool + Send + Sync>>,
    #[allow(clippy::type_complexity)]
    on_event: Option<Box<dyn Fn(LoopEvent<'_>) + Send + Sync>>,
}

/// Which dialect the tool schemas are rendered in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaDialect {
    /// OpenAI's `{"type": "function", "function": {...}}` shape.
    OpenAi,
    /// Anthropic's `{"name", "description", "input_schema"}` shape.
    Anthropic,
}

impl std::fmt::Debug for AgentLoop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentLoop")
            .field("tools", &self.registry.names())
            .field("model", &self.model)
            .field("max_steps", &self.max_steps)
            .field("schema", &self.schema)
            .field("has_session", &self.session.is_some())
            .field("has_budget", &self.budget.is_some())
            .finish()
    }
}

impl AgentLoop {
    /// Creates a loop over the given tools.
    pub fn new(registry: Registry) -> Self {
        Self {
            registry,
            model: None,
            system: None,
            max_steps: 10,
            schema: SchemaDialect::OpenAi,
            session: None,
            budget: None,
            on_event: None,
        }
    }

    /// Sets the model id sent with each request.
    ///
    /// Optional: a [`Client`] configured with a default model fills this in.
    #[must_use]
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Sets the system prompt.
    #[must_use]
    pub fn with_system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    /// Sets the hard cap on model turns.
    #[must_use]
    pub fn with_max_steps(mut self, max_steps: u32) -> Self {
        self.max_steps = max_steps;
        self
    }

    /// Sets which dialect tool schemas are rendered in.
    #[must_use]
    pub fn with_schema(mut self, schema: SchemaDialect) -> Self {
        self.schema = schema;
        self
    }

    /// Shares a session with the tools, so a `done` tool can end the run.
    ///
    /// The loop checks [`Session::finished`] after each turn and stops as soon
    /// as it is set, which is why calling `finish` costs no extra request.
    #[must_use]
    pub fn with_session(mut self, session: Arc<Session>) -> Self {
        self.session = Some(session);
        self
    }

    /// Sets a budget check, run after each turn with the totals so far.
    ///
    /// Returning `false` stops the run with [`StopReason::Budget`].
    #[must_use]
    pub fn with_budget<F>(mut self, budget: F) -> Self
    where
        F: Fn(&TokenUsage) -> bool + Send + Sync + 'static,
    {
        self.budget = Some(Box::new(budget));
        self
    }

    /// Sets an event callback, for progress reporting.
    #[must_use]
    pub fn on_event<F>(mut self, on_event: F) -> Self
    where
        F: Fn(LoopEvent<'_>) + Send + Sync + 'static,
    {
        self.on_event = Some(Box::new(on_event));
        self
    }

    /// Returns the configured step cap.
    pub fn max_steps(&self) -> u32 {
        self.max_steps
    }

    /// Runs until the model stops calling tools, or a cap is hit.
    ///
    /// # Errors
    ///
    /// Returns whatever error the client returns. A tool that fails does not
    /// produce an error here: its failure becomes the result text the model
    /// sees, so the run continues.
    pub async fn run(
        &self,
        client: &dyn Completer,
        user: impl Into<String>,
    ) -> Result<RunOutcome, CosmosError> {
        let mut messages = Vec::new();
        if let Some(system) = &self.system {
            messages.push(Message::system(system.clone()));
        }
        messages.push(Message::user(user.into()));

        let schemas = self.tool_schemas();
        let mut usage = TokenUsage::default();
        let mut text = String::new();
        let mut steps_taken = 0;
        let mut reason = StopReason::Finished;

        while steps_taken < self.max_steps {
            steps_taken += 1;

            let mut req =
                CompletionRequest::new(self.model.clone().unwrap_or_default(), messages.clone());
            if !schemas.is_empty() {
                req = req.with_tools(schemas.clone());
            }

            let response = client.completion(&req).await?;
            accumulate_usage(&mut usage, &response, steps_taken);

            if !response.text().is_empty() {
                text = response.text().to_owned();
            }

            let calls = response.tool_calls().to_vec();
            if calls.is_empty() {
                self.emit(LoopEvent::Text(response.text()));
                reason = StopReason::Finished;
                break;
            }

            // The assistant turn goes back with its tool_calls intact: a
            // provider rejects a tool result whose call it cannot see.
            messages.push(response.to_assistant_message());

            let results = self.dispatch_all(&calls).await;
            for (call, result) in calls.iter().zip(results) {
                self.emit(LoopEvent::Tool {
                    name: &call.name,
                    input: &call.input,
                    result: &result,
                });
                messages.push(Message::tool_result(call.id.clone(), result));
            }

            // A tool that called `finish` ends the run here rather than at the
            // next model turn, so a `done` call does not cost an extra request.
            if self.session.as_ref().is_some_and(|s| s.finished()) {
                reason = StopReason::Finished;
                break;
            }

            if let Some(budget) = &self.budget {
                if !budget(&usage) {
                    reason = StopReason::Budget;
                    break;
                }
            }

            if steps_taken >= self.max_steps {
                reason = StopReason::Steps;
            }
        }

        Ok(RunOutcome {
            text,
            steps_taken,
            reason,
            usage,
            messages,
        })
    }

    /// Renders every registered tool in the configured dialect.
    fn tool_schemas(&self) -> Vec<Value> {
        self.registry
            .all()
            .iter()
            .map(|tool| match self.schema {
                SchemaDialect::OpenAi => tool.to_openai_schema(),
                SchemaDialect::Anthropic => tool.to_anthropic_schema(),
            })
            .collect()
    }

    /// Runs every call in a turn concurrently, in the order the model asked.
    ///
    /// A model that asks for four independent lookups should not wait for them
    /// serially. [`Executor::execute_all`] preserves order, so results still
    /// line up with the calls that produced them.
    async fn dispatch_all(&self, calls: &[ToolCall]) -> Vec<String> {
        let mut known = Vec::with_capacity(calls.len());
        let mut unknown = Vec::new();

        for (index, call) in calls.iter().enumerate() {
            match self.registry.get(&call.name) {
                Some(tool) => known.push((index, tool, call.input.clone())),
                None => unknown.push(index),
            }
        }

        let batch: Vec<_> = known
            .iter()
            .map(|(_, tool, input)| (*tool, input.clone()))
            .collect();
        let executed = Executor::execute_all(batch, 8).await;

        let mut results = vec![String::new(); calls.len()];
        for (slot, outcome) in known.iter().zip(executed) {
            results[slot.0] = match outcome {
                Ok(value) => stringify(&value),
                // A tool failure is information the model can act on, not a
                // reason to abandon the run.
                Err(err) => format!("Tool error: {err}"),
            };
        }
        for index in unknown {
            results[index] = format!(
                "No such tool: {}. Use only the tools you were given.",
                calls[index].name
            );
        }
        results
    }

    /// Hands an event to the callback, if one is set.
    fn emit(&self, event: LoopEvent<'_>) {
        if let Some(callback) = &self.on_event {
            callback(event);
        }
    }
}

/// Renders a tool result as the string the model will read.
///
/// A JSON string is unwrapped rather than re-quoted: double-encoding it only
/// wastes tokens and confuses the model.
fn stringify(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Adds a response's reported usage to the running totals.
///
/// Cost accumulates separately from tokens: a turn that reports counts but no
/// price must not reset the running cost to zero, and a run where no turn ever
/// reports one must end with `None` rather than `Some(0.0)` — the difference
/// between "this run was free" and "nobody told us".
fn accumulate_usage(usage: &mut TokenUsage, response: &CompletionResponse, steps: u32) {
    usage.steps = steps;
    if let Some(reported) = &response.usage {
        usage.prompt_tokens += u64::from(reported.prompt_tokens);
        usage.completion_tokens += u64::from(reported.completion_tokens);
        usage.total_tokens += u64::from(reported.total_tokens);
        if let Some(cost) = reported.cost {
            usage.cost = Some(usage.cost.unwrap_or(0.0) + cost);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ParameterType, ToolDefinition};
    use cosmos_llm::{Choice, Usage};
    use serde_json::json;
    use std::sync::Mutex;

    /// A client that replays a scripted list of responses.
    struct ScriptedClient {
        responses: Mutex<Vec<CompletionResponse>>,
        requests: Mutex<Vec<CompletionRequest>>,
    }

    impl ScriptedClient {
        fn new(responses: Vec<CompletionResponse>) -> Self {
            Self {
                // Reversed so `pop` yields them in order.
                responses: Mutex::new(responses.into_iter().rev().collect()),
                requests: Mutex::new(Vec::new()),
            }
        }

        fn request_count(&self) -> usize {
            self.requests.lock().unwrap().len()
        }
    }

    impl Completer for ScriptedClient {
        fn completion<'a>(
            &'a self,
            req: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse, CosmosError>> + Send + 'a>>
        {
            self.requests.lock().unwrap().push(req.clone());
            let next = self.responses.lock().unwrap().pop();
            Box::pin(async move {
                next.ok_or_else(|| CosmosError::Api {
                    provider: "scripted".to_owned(),
                    message: "script exhausted".to_owned(),
                })
            })
        }
    }

    fn reply(text: &str) -> CompletionResponse {
        CompletionResponse {
            id: None,
            model: None,
            choices: vec![Choice {
                index: 0,
                message: Message::assistant(text),
                finish_reason: Some("stop".to_owned()),
                tool_calls: vec![],
            }],
            usage: None,
        }
    }

    fn calls(text: &str, tool_calls: Vec<ToolCall>) -> CompletionResponse {
        CompletionResponse {
            id: None,
            model: None,
            choices: vec![Choice {
                index: 0,
                message: Message::assistant(text),
                finish_reason: Some("tool_calls".to_owned()),
                tool_calls,
            }],
            usage: None,
        }
    }

    fn call(id: &str, name: &str, input: Value) -> ToolCall {
        ToolCall {
            id: id.to_owned(),
            name: name.to_owned(),
            input,
        }
    }

    fn echo_registry() -> Registry {
        let mut registry = Registry::new();
        registry.register(
            ToolDefinition::new("echo")
                .param("value", ParameterType::String, true, "text")
                .handler(|p| {
                    Ok(json!(format!(
                        "echoed:{}",
                        p["value"].as_str().unwrap_or("")
                    )))
                }),
        );
        registry
    }

    #[tokio::test]
    async fn returns_immediately_when_no_tool_use() {
        let client = ScriptedClient::new(vec![reply("final answer")]);
        let agent = AgentLoop::new(echo_registry()).with_model("m");

        let outcome = agent.run(&client, "hi").await.unwrap();

        assert_eq!(outcome.text, "final answer");
        assert_eq!(outcome.steps_taken, 1);
        assert_eq!(outcome.reason, StopReason::Finished);
        assert_eq!(client.request_count(), 1);
    }

    #[tokio::test]
    async fn dispatches_a_tool_call_and_feeds_the_result_back() {
        let client = ScriptedClient::new(vec![
            calls("", vec![call("call_1", "echo", json!({"value": "hello"}))]),
            reply("done"),
        ]);
        let agent = AgentLoop::new(echo_registry()).with_model("m");

        let outcome = agent.run(&client, "hi").await.unwrap();

        assert_eq!(outcome.text, "done");
        assert_eq!(outcome.steps_taken, 2);
        let result = outcome
            .messages
            .iter()
            .find(|m| m.is_tool_result())
            .expect("a tool result was appended");
        assert_eq!(result.content, "echoed:hello");
        assert_eq!(result.tool_call_id.as_deref(), Some("call_1"));
    }

    #[tokio::test]
    async fn assistant_turn_is_replayed_with_its_tool_calls() {
        let client = ScriptedClient::new(vec![
            calls(
                "thinking",
                vec![call("call_1", "echo", json!({"value": "x"}))],
            ),
            reply("done"),
        ]);
        let agent = AgentLoop::new(echo_registry()).with_model("m");

        let outcome = agent.run(&client, "hi").await.unwrap();

        let assistant = outcome
            .messages
            .iter()
            .find(|m| m.role == "assistant")
            .expect("the assistant turn was replayed");
        assert_eq!(assistant.content, "thinking");
        assert_eq!(assistant.tool_calls.len(), 1);
        assert_eq!(assistant.tool_calls[0].id, "call_1");
    }

    #[tokio::test]
    async fn stops_at_max_steps_while_the_model_keeps_calling_tools() {
        let responses = (0..5)
            .map(|_| calls("mid", vec![call("c", "echo", json!({"value": "x"}))]))
            .collect();
        let client = ScriptedClient::new(responses);
        let agent = AgentLoop::new(echo_registry())
            .with_model("m")
            .with_max_steps(3);

        let outcome = agent.run(&client, "hi").await.unwrap();

        assert_eq!(outcome.steps_taken, 3);
        assert_eq!(outcome.reason, StopReason::Steps);
        assert_eq!(client.request_count(), 3);
    }

    #[tokio::test]
    async fn unknown_tool_is_reported_to_the_model_not_raised() {
        let client = ScriptedClient::new(vec![
            calls("", vec![call("call_1", "nonexistent", json!({}))]),
            reply("recovered"),
        ]);
        let agent = AgentLoop::new(echo_registry()).with_model("m");

        let outcome = agent.run(&client, "hi").await.unwrap();

        let result = outcome
            .messages
            .iter()
            .find(|m| m.is_tool_result())
            .unwrap();
        assert!(result.content.contains("No such tool: nonexistent"));
        assert_eq!(outcome.text, "recovered");
    }

    #[tokio::test]
    async fn a_failing_tool_becomes_a_message_the_model_can_act_on() {
        let mut registry = Registry::new();
        registry.register(ToolDefinition::new("boom").handler(|_| Err("exploded".to_owned())));
        let client = ScriptedClient::new(vec![
            calls("", vec![call("call_1", "boom", json!({}))]),
            reply("recovered"),
        ]);
        let agent = AgentLoop::new(registry).with_model("m");

        let outcome = agent.run(&client, "hi").await.unwrap();

        let result = outcome
            .messages
            .iter()
            .find(|m| m.is_tool_result())
            .unwrap();
        assert!(result.content.contains("Tool error"));
        assert!(result.content.contains("exploded"));
        assert_eq!(outcome.reason, StopReason::Finished);
    }

    #[tokio::test]
    async fn session_finish_ends_the_run_without_another_request() {
        let session = Arc::new(Session::new());
        let for_tool = Arc::clone(&session);

        let mut registry = Registry::new();
        registry.register(ToolDefinition::new("done").handler(move |_| {
            for_tool.finish(Some("all set"));
            Ok(json!("done"))
        }));

        let client = ScriptedClient::new(vec![
            calls("", vec![call("call_1", "done", json!({}))]),
            reply("never reached"),
        ]);
        let agent = AgentLoop::new(registry)
            .with_model("m")
            .with_session(Arc::clone(&session));

        let outcome = agent.run(&client, "hi").await.unwrap();

        assert_eq!(outcome.reason, StopReason::Finished);
        assert_eq!(outcome.steps_taken, 1);
        assert_eq!(client.request_count(), 1);
        assert_eq!(session.notes(), vec!["all set".to_string()]);
    }

    #[tokio::test]
    async fn budget_callback_stops_the_run() {
        let responses = (0..5)
            .map(|_| calls("mid", vec![call("c", "echo", json!({"value": "x"}))]))
            .collect();
        let client = ScriptedClient::new(responses);
        let agent = AgentLoop::new(echo_registry())
            .with_model("m")
            .with_max_steps(10)
            .with_budget(|usage| usage.steps < 2);

        let outcome = agent.run(&client, "hi").await.unwrap();

        assert_eq!(outcome.reason, StopReason::Budget);
        assert_eq!(outcome.steps_taken, 2);
    }

    #[tokio::test]
    async fn budget_is_not_consulted_when_the_model_stops_on_its_own() {
        let client = ScriptedClient::new(vec![reply("done")]);
        let seen = Arc::new(Mutex::new(false));
        let flag = Arc::clone(&seen);

        let agent = AgentLoop::new(echo_registry())
            .with_model("m")
            .with_budget(move |_| {
                *flag.lock().unwrap() = true;
                true
            });

        agent.run(&client, "hi").await.unwrap();

        assert!(!*seen.lock().unwrap());
    }

    #[tokio::test]
    async fn usage_accumulates_across_turns() {
        let mut first = calls("", vec![call("c", "echo", json!({"value": "x"}))]);
        first.usage = Some(Usage::from_tokens(10, 5));
        let mut second = reply("done");
        second.usage = Some(Usage::from_tokens(20, 4));

        let client = ScriptedClient::new(vec![first, second]);
        let agent = AgentLoop::new(echo_registry()).with_model("m");

        let outcome = agent.run(&client, "hi").await.unwrap();

        assert_eq!(outcome.usage.prompt_tokens, 30);
        assert_eq!(outcome.usage.completion_tokens, 9);
        assert_eq!(outcome.usage.total_tokens, 39);
        assert_eq!(outcome.usage.steps, 2);
    }

    #[tokio::test]
    async fn cost_accumulates_across_turns() {
        let mut first = calls("", vec![call("c", "echo", json!({"value": "x"}))]);
        first.usage = Some(Usage::from_tokens(10, 5).with_cost(0.001));
        let mut second = reply("done");
        second.usage = Some(Usage::from_tokens(20, 4).with_cost(0.002));

        let client = ScriptedClient::new(vec![first, second]);
        let agent = AgentLoop::new(echo_registry()).with_model("m");

        let outcome = agent.run(&client, "hi").await.unwrap();

        let cost = outcome.usage.cost.expect("both turns reported a price");
        assert!((cost - 0.003).abs() < 1e-9, "got {cost}");
    }

    #[tokio::test]
    async fn cost_stays_unknown_when_no_turn_reports_one() {
        let mut only = reply("done");
        only.usage = Some(Usage::from_tokens(10, 5));

        let client = ScriptedClient::new(vec![only]);
        let agent = AgentLoop::new(echo_registry()).with_model("m");

        let outcome = agent.run(&client, "hi").await.unwrap();

        // Unknown, not free. A run that was billed but never told us must not
        // read as zero dollars.
        assert_eq!(outcome.usage.cost, None);
        assert_eq!(outcome.usage.cost_or_zero(), 0.0);
    }

    #[tokio::test]
    async fn a_priceless_turn_does_not_reset_the_running_cost() {
        let mut first = calls("", vec![call("c", "echo", json!({"value": "x"}))]);
        first.usage = Some(Usage::from_tokens(10, 5).with_cost(0.001));
        // Second turn reports counts but no price.
        let mut second = reply("done");
        second.usage = Some(Usage::from_tokens(20, 4));

        let client = ScriptedClient::new(vec![first, second]);
        let agent = AgentLoop::new(echo_registry()).with_model("m");

        let outcome = agent.run(&client, "hi").await.unwrap();

        let cost = outcome.usage.cost.expect("the first turn reported a price");
        assert!((cost - 0.001).abs() < 1e-9, "got {cost}");
        assert_eq!(outcome.usage.total_tokens, 39);
    }

    #[tokio::test]
    async fn budget_can_stop_a_run_on_a_dollar_ceiling() {
        let responses = (0..5)
            .map(|_| {
                let mut turn = calls("mid", vec![call("c", "echo", json!({"value": "x"}))]);
                turn.usage = Some(Usage::from_tokens(10, 5).with_cost(0.004));
                turn
            })
            .collect();
        let client = ScriptedClient::new(responses);
        let agent = AgentLoop::new(echo_registry())
            .with_model("m")
            .with_max_steps(10)
            .with_budget(|usage| usage.cost_or_zero() < 0.01);

        let outcome = agent.run(&client, "hi").await.unwrap();

        assert_eq!(outcome.reason, StopReason::Budget);
        // Stops on the turn that crosses $0.01: 0.004, 0.008, 0.012.
        assert_eq!(outcome.steps_taken, 3);
    }

    #[tokio::test]
    async fn usage_tolerates_a_provider_that_reports_none() {
        let client = ScriptedClient::new(vec![reply("done")]);
        let agent = AgentLoop::new(echo_registry()).with_model("m");

        let outcome = agent.run(&client, "hi").await.unwrap();

        assert_eq!(outcome.usage.total_tokens, 0);
        assert_eq!(outcome.usage.steps, 1);
    }

    #[tokio::test]
    async fn several_calls_in_one_turn_keep_their_order() {
        let client = ScriptedClient::new(vec![
            calls(
                "",
                vec![
                    call("c1", "echo", json!({"value": "one"})),
                    call("c2", "echo", json!({"value": "two"})),
                    call("c3", "echo", json!({"value": "three"})),
                ],
            ),
            reply("done"),
        ]);
        let agent = AgentLoop::new(echo_registry()).with_model("m");

        let outcome = agent.run(&client, "hi").await.unwrap();

        let results: Vec<_> = outcome
            .messages
            .iter()
            .filter(|m| m.is_tool_result())
            .map(|m| m.content.clone())
            .collect();
        assert_eq!(results, vec!["echoed:one", "echoed:two", "echoed:three"]);
    }

    #[tokio::test]
    async fn a_mix_of_known_and_unknown_tools_lines_up() {
        let client = ScriptedClient::new(vec![
            calls(
                "",
                vec![
                    call("c1", "echo", json!({"value": "one"})),
                    call("c2", "missing", json!({})),
                    call("c3", "echo", json!({"value": "three"})),
                ],
            ),
            reply("done"),
        ]);
        let agent = AgentLoop::new(echo_registry()).with_model("m");

        let outcome = agent.run(&client, "hi").await.unwrap();

        let results: Vec<_> = outcome
            .messages
            .iter()
            .filter(|m| m.is_tool_result())
            .map(|m| m.content.clone())
            .collect();
        assert_eq!(results[0], "echoed:one");
        assert!(results[1].contains("No such tool: missing"));
        assert_eq!(results[2], "echoed:three");
    }

    #[tokio::test]
    async fn events_report_tool_calls_and_final_text() {
        let client = ScriptedClient::new(vec![
            calls("", vec![call("c1", "echo", json!({"value": "hello"}))]),
            reply("all done"),
        ]);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);

        let agent = AgentLoop::new(echo_registry())
            .with_model("m")
            .on_event(move |event| {
                let entry = match event {
                    LoopEvent::Text(text) => format!("text:{text}"),
                    LoopEvent::Tool { name, result, .. } => format!("tool:{name}:{result}"),
                };
                sink.lock().unwrap().push(entry);
            });

        agent.run(&client, "hi").await.unwrap();

        let events = seen.lock().unwrap().clone();
        assert_eq!(events, vec!["tool:echo:echoed:hello", "text:all done"]);
    }

    #[tokio::test]
    async fn system_prompt_leads_the_message_history() {
        let client = ScriptedClient::new(vec![reply("ok")]);
        let agent = AgentLoop::new(echo_registry())
            .with_model("m")
            .with_system("be brief");

        let outcome = agent.run(&client, "hi").await.unwrap();

        assert_eq!(outcome.messages[0].role, "system");
        assert_eq!(outcome.messages[0].content, "be brief");
        assert_eq!(outcome.messages[1].role, "user");
    }

    #[tokio::test]
    async fn tool_schemas_are_sent_in_the_configured_dialect() {
        let client = ScriptedClient::new(vec![reply("ok")]);
        let agent = AgentLoop::new(echo_registry())
            .with_model("m")
            .with_schema(SchemaDialect::Anthropic);

        agent.run(&client, "hi").await.unwrap();

        let sent = client.requests.lock().unwrap()[0].tools.clone().unwrap();
        assert!(sent[0].get("input_schema").is_some(), "anthropic dialect");
        assert!(sent[0].get("function").is_none());
    }

    #[tokio::test]
    async fn a_client_error_ends_the_run_as_an_error() {
        let client = ScriptedClient::new(vec![]);
        let agent = AgentLoop::new(echo_registry()).with_model("m");

        assert!(agent.run(&client, "hi").await.is_err());
    }

    #[test]
    fn stringify_does_not_requote_a_json_string() {
        assert_eq!(stringify(&json!("done")), "done");
        assert_eq!(stringify(&json!({"a": 1})), r#"{"a":1}"#);
    }
}

// Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
