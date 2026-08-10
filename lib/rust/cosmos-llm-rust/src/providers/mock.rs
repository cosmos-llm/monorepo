//! A scripted provider for testing, with no network involved.
//!
//! Every consumer of this crate needs one of these to test its own code, which
//! is why it lives here rather than in a separate testing crate — a
//! `dev-dependency` on a crate that itself depends on `cosmos-llm` would be a
//! cycle.
//!
//! Enabled by the `mock` feature, which is on by default.
//!
//! # Examples
//!
//! ```
//! use cosmos_llm::providers::mock::MockProvider;
//! use cosmos_llm::providers::Provider;
//! use cosmos_llm::{CompletionRequest, Message};
//!
//! # tokio_test::block_on(async {
//! let provider = MockProvider::new()
//!     .respond("first answer")
//!     .respond("second answer");
//!
//! let req = CompletionRequest::new("mock-model", vec![Message::user("hi")]);
//! assert_eq!(provider.completion(&req).await.unwrap().text(), "first answer");
//! assert_eq!(provider.completion(&req).await.unwrap().text(), "second answer");
//!
//! // Every request is recorded.
//! assert_eq!(provider.requests().len(), 2);
//! # })
//! ```

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;

use crate::error::CosmosError;
use crate::providers::{CompletionStream, Provider};
use crate::types::{
    Choice, CompletionRequest, CompletionResponse, Message, StreamChunk, ToolCall, ToolCallDelta,
    Usage,
};

/// Name this provider reports from [`Provider::name`] and attaches to errors.
const PROVIDER_NAME: &str = "mock";

/// How a streamed response is split into chunks.
///
/// The mock's streaming path must reconstruct, through
/// [`StreamAccumulator`](crate::StreamAccumulator), exactly the response its
/// non-streaming path returns. Varying the split is how a test proves its own
/// accumulation is not accidentally relying on chunk boundaries — including the
/// case that breaks real accumulators, a tool-call argument cut mid-token.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Chunking {
    /// One chunk for the whole response. The trivial case.
    #[default]
    Whole,
    /// One chunk per character, tool-call arguments included.
    ///
    /// The most adversarial setting: a JSON argument is cut between every pair
    /// of characters, so `{"city":"Boston"}` arrives as seventeen fragments.
    PerCharacter,
    /// One chunk per whitespace-separated word, with the space kept.
    PerWord,
    /// Chunks of at most `n` characters.
    ///
    /// `Fixed(0)` behaves as [`Chunking::Whole`]; a zero-length chunk would
    /// stream forever.
    Fixed(usize),
}

impl Chunking {
    /// Splits text into the fragments this strategy calls for.
    ///
    /// Fragments always concatenate back to the input, which is what makes the
    /// streaming and non-streaming paths equivalent.
    fn split(&self, text: &str) -> Vec<String> {
        if text.is_empty() {
            return Vec::new();
        }
        match self {
            Self::Whole => vec![text.to_owned()],
            // Character boundaries, not byte boundaries: splitting a multi-byte
            // character would produce fragments that do not reassemble.
            Self::PerCharacter => text.chars().map(String::from).collect(),
            Self::PerWord => split_keeping_spaces(text),
            Self::Fixed(0) => vec![text.to_owned()],
            Self::Fixed(n) => {
                let chars: Vec<char> = text.chars().collect();
                chars.chunks(*n).map(|c| c.iter().collect()).collect()
            }
        }
    }
}

/// Splits on whitespace while preserving it, so fragments rejoin exactly.
fn split_keeping_spaces(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_space = false;

    for ch in text.chars() {
        let is_space = ch.is_whitespace();
        // A word ends when whitespace begins; the whitespace joins the word
        // that follows it, keeping the count equal to the word count.
        if is_space && !in_space && !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
        current.push(ch);
        in_space = is_space;
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Builds a fresh error each time it is called.
///
/// Errors are scripted as builders rather than values because `CosmosError` is
/// not `Clone` — it can hold a `reqwest::Error` — and a step may be replayed
/// (see [`MockProvider::fail_times`]).
type ErrorFn = Arc<dyn Fn() -> CosmosError + Send + Sync>;

/// One scripted outcome for a single request.
#[derive(Clone)]
enum Step {
    /// Return this response.
    Respond(Box<CompletionResponse>),
    /// Fail with the error this builds.
    Fail(ErrorFn),
    /// Emit `after` chunks of the response, then fail.
    FailMidStream {
        response: Box<CompletionResponse>,
        after: usize,
        error: ErrorFn,
    },
    /// Compute a response from the request itself.
    Canned(Canned),
}

impl std::fmt::Debug for Step {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Respond(r) => f.debug_tuple("Respond").field(&r.text()).finish(),
            Self::Fail(_) => f.write_str("Fail(<error>)"),
            Self::FailMidStream { after, .. } => f
                .debug_struct("FailMidStream")
                .field("after", after)
                .finish_non_exhaustive(),
            Self::Canned(c) => f.debug_tuple("Canned").field(c).finish(),
        }
    }
}

/// A response derived from the request rather than scripted in advance.
///
/// For tests that exercise plumbing and do not care what the model "said".
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Canned {
    /// Return the content of the last user message.
    Echo,
    /// Return the number of messages in the request, as a decimal string.
    Count,
    /// Return a fixed string, however many times it is called.
    Fixed(String),
}

/// Shared mutable state, so a `&self` provider can still consume its script.
///
/// [`Provider`] takes `&self`, and a mock has to advance through its script and
/// record requests anyway, so the interior mutability is unavoidable. The
/// `Mutex` is never held across an `await`.
#[derive(Debug, Default)]
struct Shared {
    requests: Mutex<Vec<CompletionRequest>>,
}

/// A provider that returns scripted responses instead of calling an API.
///
/// Build a script with [`MockProvider::respond`],
/// [`MockProvider::respond_with_tool_call`], [`MockProvider::fail_with`], and
/// friends. Steps are consumed in order; running past the end is an error
/// rather than a silent replay of the last response, because a test that
/// unknowingly reuses a response hides the bug it was written to catch.
///
/// # Examples
///
/// ```
/// use cosmos_llm::providers::mock::MockProvider;
/// use cosmos_llm::providers::Provider;
/// use cosmos_llm::{CompletionRequest, Message};
/// use serde_json::json;
///
/// # tokio_test::block_on(async {
/// let provider = MockProvider::new()
///     .respond_with_tool_call("web_search", json!({"query": "rust"}))
///     .respond("here is what I found");
///
/// let req = CompletionRequest::new("mock-model", vec![Message::user("search")]);
///
/// let first = provider.completion(&req).await.unwrap();
/// assert!(first.tool_use());
/// assert_eq!(first.tool_calls()[0].name, "web_search");
///
/// let second = provider.completion(&req).await.unwrap();
/// assert_eq!(second.text(), "here is what I found");
/// # })
/// ```
pub struct MockProvider {
    steps: Vec<Step>,
    cursor: AtomicUsize,
    shared: Arc<Shared>,
    chunking: Chunking,
    delay: Option<Duration>,
    models: Vec<String>,
    next_call_id: AtomicUsize,
}

impl std::fmt::Debug for MockProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockProvider")
            .field("steps", &self.steps.len())
            .field("consumed", &self.cursor.load(Ordering::SeqCst))
            .field("chunking", &self.chunking)
            .field("delay", &self.delay)
            .finish()
    }
}

impl Default for MockProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl MockProvider {
    /// Creates a provider with an empty script.
    ///
    /// Calling it before adding a step is an error — see
    /// [`MockProvider::completion`](Provider::completion).
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::mock::MockProvider;
    ///
    /// let provider = MockProvider::new();
    /// assert!(provider.requests().is_empty());
    /// ```
    pub fn new() -> Self {
        Self {
            steps: Vec::new(),
            cursor: AtomicUsize::new(0),
            shared: Arc::new(Shared::default()),
            chunking: Chunking::default(),
            delay: None,
            models: vec!["mock-model".to_owned()],
            next_call_id: AtomicUsize::new(1),
        }
    }

    /// Appends a step returning `text` as the assistant's reply.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::mock::MockProvider;
    ///
    /// let provider = MockProvider::new().respond("hello");
    /// assert_eq!(provider.remaining(), 1);
    /// ```
    pub fn respond(mut self, text: impl Into<String>) -> Self {
        let text = text.into();
        let usage = estimated_usage(&text);
        self.steps.push(Step::Respond(Box::new(response_of(
            text,
            Vec::new(),
            "stop",
            Some(usage),
        ))));
        self
    }

    /// Appends a step returning a single tool call.
    ///
    /// The call id is generated (`call_1`, `call_2`, …) so a test does not have
    /// to invent one, and matches what a real provider does closely enough for
    /// a tool loop to round-trip it.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::mock::MockProvider;
    /// use serde_json::json;
    ///
    /// let provider = MockProvider::new()
    ///     .respond_with_tool_call("get_weather", json!({"city": "Boston"}));
    /// assert_eq!(provider.remaining(), 1);
    /// ```
    pub fn respond_with_tool_call(self, name: impl Into<String>, input: Value) -> Self {
        let id = format!("call_{}", self.next_call_id.fetch_add(1, Ordering::SeqCst));
        self.respond_with_tool_calls(vec![ToolCall {
            id,
            name: name.into(),
            input,
        }])
    }

    /// Appends a step returning several tool calls at once.
    ///
    /// Parallel tool calls are what every current model does, and they are
    /// where a loop that ignores call ids breaks, so testing against them
    /// matters.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::mock::MockProvider;
    /// use cosmos_llm::ToolCall;
    /// use serde_json::json;
    ///
    /// let provider = MockProvider::new().respond_with_tool_calls(vec![
    ///     ToolCall { id: "a".into(), name: "search".into(), input: json!({}) },
    ///     ToolCall { id: "b".into(), name: "fetch".into(), input: json!({}) },
    /// ]);
    /// assert_eq!(provider.remaining(), 1);
    /// ```
    pub fn respond_with_tool_calls(mut self, calls: Vec<ToolCall>) -> Self {
        self.steps.push(Step::Respond(Box::new(response_of(
            String::new(),
            calls,
            "tool_calls",
            Some(Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            }),
        ))));
        self
    }

    /// Appends a step returning both text and tool calls.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::mock::MockProvider;
    /// use cosmos_llm::ToolCall;
    /// use serde_json::json;
    ///
    /// let provider = MockProvider::new().respond_with_text_and_tool_calls(
    ///     "Let me check.",
    ///     vec![ToolCall { id: "a".into(), name: "search".into(), input: json!({}) }],
    /// );
    /// assert_eq!(provider.remaining(), 1);
    /// ```
    pub fn respond_with_text_and_tool_calls(
        mut self,
        text: impl Into<String>,
        calls: Vec<ToolCall>,
    ) -> Self {
        let text = text.into();
        let usage = estimated_usage(&text);
        self.steps.push(Step::Respond(Box::new(response_of(
            text,
            calls,
            "tool_calls",
            Some(usage),
        ))));
        self
    }

    /// Appends a step returning a full [`CompletionResponse`] verbatim.
    ///
    /// For the cases the other builders do not cover — an unusual
    /// `finish_reason`, several choices, specific token counts.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::mock::MockProvider;
    /// use cosmos_llm::{Choice, CompletionResponse, Message};
    ///
    /// let provider = MockProvider::new().respond_with(CompletionResponse {
    ///     id: Some("resp_1".into()),
    ///     model: Some("mock-model".into()),
    ///     choices: vec![Choice {
    ///         index: 0,
    ///         message: Message::assistant("truncated"),
    ///         finish_reason: Some("length".into()),
    ///         tool_calls: vec![],
    ///     }],
    ///     usage: None,
    /// });
    /// assert_eq!(provider.remaining(), 1);
    /// ```
    pub fn respond_with(mut self, response: CompletionResponse) -> Self {
        self.steps.push(Step::Respond(Box::new(response)));
        self
    }

    /// Appends a step computing its response from the request.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::mock::{Canned, MockProvider};
    /// use cosmos_llm::providers::Provider;
    /// use cosmos_llm::{CompletionRequest, Message};
    ///
    /// # tokio_test::block_on(async {
    /// let provider = MockProvider::new().respond_canned(Canned::Echo);
    /// let req = CompletionRequest::new("mock-model", vec![Message::user("say this back")]);
    /// assert_eq!(provider.completion(&req).await.unwrap().text(), "say this back");
    /// # })
    /// ```
    pub fn respond_canned(mut self, canned: Canned) -> Self {
        self.steps.push(Step::Canned(canned));
        self
    }

    /// Appends a step that fails with `error`.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use cosmos_llm::providers::mock::MockProvider;
    /// use cosmos_llm::CosmosError;
    ///
    /// let provider = MockProvider::new().fail_with(|| CosmosError::RateLimit {
    ///     provider: "mock".into(),
    ///     message: "slow down".into(),
    ///     retry_after: Some(Duration::from_secs(20)),
    /// });
    /// assert_eq!(provider.remaining(), 1);
    /// ```
    pub fn fail_with<F>(mut self, error: F) -> Self
    where
        F: Fn() -> CosmosError + Send + Sync + 'static,
    {
        self.steps.push(Step::Fail(Arc::new(error)));
        self
    }

    /// Appends `n` steps that all fail the same way.
    ///
    /// Pair with [`MockProvider::respond`] to script a retry test: two
    /// failures, then success, asserts that a retry policy makes exactly three
    /// attempts.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::mock::MockProvider;
    /// use cosmos_llm::CosmosError;
    ///
    /// let provider = MockProvider::new()
    ///     .fail_times(2, || CosmosError::Server {
    ///         provider: "mock".into(),
    ///         status: 503,
    ///         message: "overloaded".into(),
    ///     })
    ///     .respond("ok");
    /// assert_eq!(provider.remaining(), 3);
    /// ```
    pub fn fail_times<F>(mut self, n: usize, error: F) -> Self
    where
        F: Fn() -> CosmosError + Send + Sync + 'static,
    {
        let error = Arc::new(error);
        for _ in 0..n {
            self.steps.push(Step::Fail(error.clone()));
        }
        self
    }

    /// Appends a step that streams `after` chunks and then fails.
    ///
    /// A stream that dies partway is not the same failure as a request that
    /// never opened: the caller has already seen output. Only the streaming
    /// path honours this; the non-streaming path returns the error directly.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::mock::{Chunking, MockProvider};
    /// use cosmos_llm::CosmosError;
    ///
    /// let provider = MockProvider::new()
    ///     .with_chunking(Chunking::PerWord)
    ///     .fail_mid_stream_after("one two three four", 2, || CosmosError::Streaming {
    ///         provider: "mock".into(),
    ///         message: "connection reset".into(),
    ///     });
    /// assert_eq!(provider.remaining(), 1);
    /// ```
    pub fn fail_mid_stream_after<F>(
        mut self,
        text: impl Into<String>,
        after: usize,
        error: F,
    ) -> Self
    where
        F: Fn() -> CosmosError + Send + Sync + 'static,
    {
        let text = text.into();
        let usage = estimated_usage(&text);
        self.steps.push(Step::FailMidStream {
            response: Box::new(response_of(text, Vec::new(), "stop", Some(usage))),
            after,
            error: Arc::new(error),
        });
        self
    }

    /// Sets how streamed responses are split.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::mock::{Chunking, MockProvider};
    ///
    /// let provider = MockProvider::new()
    ///     .with_chunking(Chunking::PerCharacter)
    ///     .respond("hello");
    /// assert_eq!(provider.chunking(), &Chunking::PerCharacter);
    /// ```
    pub fn with_chunking(mut self, chunking: Chunking) -> Self {
        self.chunking = chunking;
        self
    }

    /// Returns the configured chunking strategy.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::mock::{Chunking, MockProvider};
    ///
    /// assert_eq!(MockProvider::new().chunking(), &Chunking::Whole);
    /// ```
    pub fn chunking(&self) -> &Chunking {
        &self.chunking
    }

    /// Delays every response by `delay`.
    ///
    /// For timeout and concurrency tests. Applied before the response is
    /// produced, and before a scripted failure, since a real provider's latency
    /// precedes its error too.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use cosmos_llm::providers::mock::MockProvider;
    ///
    /// let provider = MockProvider::new()
    ///     .with_delay(Duration::from_millis(50))
    ///     .respond("slow");
    /// assert_eq!(provider.remaining(), 1);
    /// ```
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = Some(delay);
        self
    }

    /// Sets the list returned by [`Provider::models`].
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::mock::MockProvider;
    /// use cosmos_llm::providers::Provider;
    ///
    /// # tokio_test::block_on(async {
    /// let provider = MockProvider::new().with_models(vec!["a".into(), "b".into()]);
    /// assert_eq!(provider.models().await.unwrap().len(), 2);
    /// # })
    /// ```
    pub fn with_models(mut self, models: Vec<String>) -> Self {
        self.models = models;
        self
    }

    /// Returns every request this provider has received, in order.
    ///
    /// This is how a test asserts on context assembly and trimming without a
    /// network: build the request, send it, inspect what arrived.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::mock::MockProvider;
    /// use cosmos_llm::providers::Provider;
    /// use cosmos_llm::{CompletionRequest, Message};
    ///
    /// # tokio_test::block_on(async {
    /// let provider = MockProvider::new().respond("ok");
    /// let req = CompletionRequest::new("mock-model", vec![Message::user("hi")])
    ///     .with_temperature(0.2);
    /// provider.completion(&req).await.unwrap();
    ///
    /// let seen = provider.requests();
    /// assert_eq!(seen[0].messages.len(), 1);
    /// assert_eq!(seen[0].temperature, Some(0.2));
    /// # })
    /// ```
    pub fn requests(&self) -> Vec<CompletionRequest> {
        self.shared
            .requests
            .lock()
            .expect("mock request log poisoned")
            .clone()
    }

    /// Returns the most recent request, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::mock::MockProvider;
    ///
    /// assert!(MockProvider::new().last_request().is_none());
    /// ```
    pub fn last_request(&self) -> Option<CompletionRequest> {
        self.shared
            .requests
            .lock()
            .expect("mock request log poisoned")
            .last()
            .cloned()
    }

    /// Returns how many scripted steps have not been consumed.
    ///
    /// A test that finishes with steps remaining usually means the code under
    /// test made fewer calls than expected, which is worth asserting on.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::mock::MockProvider;
    ///
    /// let provider = MockProvider::new().respond("a").respond("b");
    /// assert_eq!(provider.remaining(), 2);
    /// ```
    pub fn remaining(&self) -> usize {
        self.steps
            .len()
            .saturating_sub(self.cursor.load(Ordering::SeqCst))
    }

    /// Takes the next scripted step, or reports the script as exhausted.
    ///
    /// # Errors
    ///
    /// Returns [`CosmosError::Configuration`] naming how many steps were
    /// scripted and how many were consumed. A mock that silently repeated its
    /// last response would let a test pass while the code under test looped
    /// one time too many.
    fn next_step(&self) -> Result<Step, CosmosError> {
        let index = self.cursor.fetch_add(1, Ordering::SeqCst);
        self.steps
            .get(index)
            .cloned()
            .ok_or_else(|| CosmosError::Configuration {
                provider: PROVIDER_NAME.to_owned(),
                message: format!(
                    "mock script exhausted: {} response(s) scripted, request {} received. \
                     Add another .respond()/.fail_with() step, or check whether the code \
                     under test is making more calls than expected.",
                    self.steps.len(),
                    index + 1
                ),
            })
    }

    /// Records a request and applies the configured delay.
    async fn intercept(&self, req: &CompletionRequest) {
        self.shared
            .requests
            .lock()
            .expect("mock request log poisoned")
            .push(req.clone());

        if let Some(delay) = self.delay {
            tokio::time::sleep(delay).await;
        }
    }

    /// Resolves a step to the response it produces.
    fn resolve(
        &self,
        step: Step,
        req: &CompletionRequest,
    ) -> Result<CompletionResponse, CosmosError> {
        match step {
            Step::Respond(resp) => Ok(*resp),
            Step::Fail(build) => Err(build()),
            // A mid-stream failure has no meaning without a stream; the caller
            // used the wrong path, and returning the error is the honest answer.
            Step::FailMidStream { error, .. } => Err(error()),
            Step::Canned(canned) => Ok(canned_response(&canned, req)),
        }
    }
}

/// Builds a single-choice response.
fn response_of(
    text: String,
    tool_calls: Vec<ToolCall>,
    finish_reason: &str,
    usage: Option<Usage>,
) -> CompletionResponse {
    CompletionResponse {
        id: Some("mock-response".to_owned()),
        model: Some("mock-model".to_owned()),
        choices: vec![Choice {
            index: 0,
            message: Message::assistant(text),
            finish_reason: Some(finish_reason.to_owned()),
            tool_calls,
        }],
        usage,
    }
}

/// Estimates token counts from text length.
///
/// Four characters per token, the usual rough English ratio. Not accurate, but
/// proportional to the input, which is what a usage-accounting test needs: a
/// longer response must report more tokens.
fn estimated_usage(completion: &str) -> Usage {
    let completion_tokens = tokens_in(completion);
    Usage {
        prompt_tokens: 0,
        completion_tokens,
        total_tokens: completion_tokens,
    }
}

/// Rough token count: one per four characters, minimum one for any text.
fn tokens_in(text: &str) -> u32 {
    if text.is_empty() {
        return 0;
    }
    (text.chars().count() as u32).div_ceil(4).max(1)
}

/// Computes a [`Canned`] response against the request that triggered it.
fn canned_response(canned: &Canned, req: &CompletionRequest) -> CompletionResponse {
    let text = match canned {
        Canned::Echo => req
            .messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| m.content.clone())
            .unwrap_or_default(),
        Canned::Count => req.messages.len().to_string(),
        Canned::Fixed(s) => s.clone(),
    };

    let prompt_tokens = req.messages.iter().map(|m| tokens_in(&m.content)).sum();
    let completion_tokens = tokens_in(&text);

    let mut resp = response_of(text, Vec::new(), "stop", None);
    resp.usage = Some(Usage {
        prompt_tokens,
        completion_tokens,
        total_tokens: prompt_tokens + completion_tokens,
    });
    resp
}

/// Splits a response into the chunks a provider would stream.
///
/// Text fragments come first, then one fragment per tool-call argument slice,
/// then a final chunk carrying `finish_reason` and `usage` — the order OpenAI
/// uses, and the order an accumulator has to tolerate.
///
/// Feeding the result to a [`StreamAccumulator`](crate::StreamAccumulator)
/// reproduces `resp` exactly, apart from `id` and `model`, which streaming
/// chunks are not required to repeat.
fn chunks_for(resp: &CompletionResponse, chunking: &Chunking) -> Vec<StreamChunk> {
    let mut out = Vec::new();

    for fragment in chunking.split(resp.text()) {
        out.push(StreamChunk::text(fragment));
    }

    for (index, call) in resp.tool_calls().iter().enumerate() {
        let index = index as u32;
        // The opening fragment carries id and name and no arguments, which is
        // what both OpenAI and Anthropic send.
        out.push(StreamChunk {
            tool_calls: vec![ToolCallDelta {
                index,
                id: Some(call.id.clone()),
                name: Some(call.name.clone()),
                arguments: None,
            }],
            ..Default::default()
        });

        // Arguments are split by the same strategy as text, so
        // `Chunking::PerCharacter` cuts the JSON between every character —
        // the case that breaks a naive accumulator.
        let args = call.input.to_string();
        for fragment in chunking.split(&args) {
            out.push(StreamChunk {
                tool_calls: vec![ToolCallDelta {
                    index,
                    arguments: Some(fragment),
                    ..Default::default()
                }],
                ..Default::default()
            });
        }
    }

    let finish_reason = resp.choices.first().and_then(|c| c.finish_reason.clone());
    if finish_reason.is_some() || resp.usage.is_some() {
        out.push(StreamChunk {
            finish_reason,
            usage: resp.usage.clone(),
            ..Default::default()
        });
    }

    out
}

impl Provider for MockProvider {
    fn completion<'a>(
        &'a self,
        req: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse, CosmosError>> + Send + 'a>> {
        Box::pin(async move {
            self.intercept(req).await;
            let step = self.next_step()?;
            self.resolve(step, req)
        })
    }

    fn stream_completion<'a>(
        &'a self,
        req: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionStream, CosmosError>> + Send + 'a>> {
        Box::pin(async move {
            self.intercept(req).await;
            let step = self.next_step()?;

            let (chunks, tail) = match step {
                Step::FailMidStream {
                    response,
                    after,
                    error,
                } => {
                    let mut chunks = chunks_for(&response, &self.chunking);
                    chunks.truncate(after);
                    (chunks, Some(error))
                }
                other => (chunks_for(&self.resolve(other, req)?, &self.chunking), None),
            };

            Ok(Box::pin(async_stream::stream! {
                for chunk in chunks {
                    yield Ok(chunk);
                }
                if let Some(build) = tail {
                    yield Err(build());
                }
            }) as CompletionStream)
        })
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        PROVIDER_NAME
    }

    fn models<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>, CosmosError>> + Send + 'a>> {
        Box::pin(async move { Ok(self.models.clone()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::StreamAccumulator;
    use futures_util::StreamExt;
    use serde_json::json;

    fn req(text: &str) -> CompletionRequest {
        CompletionRequest::new("mock-model", vec![Message::user(text)])
    }

    /// Drains a stream into an accumulated response, failing on stream errors.
    async fn accumulate(provider: &MockProvider, req: &CompletionRequest) -> CompletionResponse {
        let mut stream = provider.stream_completion(req).await.unwrap();
        let mut acc = StreamAccumulator::new();
        while let Some(item) = stream.next().await {
            acc.push(&item.expect("stream yielded an error"));
        }
        acc.into_response()
    }

    #[tokio::test]
    async fn responses_are_consumed_in_order() {
        let provider = MockProvider::new().respond("first").respond("second");
        assert_eq!(
            provider.completion(&req("a")).await.unwrap().text(),
            "first"
        );
        assert_eq!(
            provider.completion(&req("b")).await.unwrap().text(),
            "second"
        );
        assert_eq!(provider.remaining(), 0);
    }

    #[tokio::test]
    async fn running_past_the_script_is_an_error_naming_the_counts() {
        let provider = MockProvider::new().respond("only one");
        provider.completion(&req("a")).await.unwrap();

        let err = provider.completion(&req("b")).await.unwrap_err();
        let text = err.to_string();
        // The message has to say how many were scripted and which request
        // overran, or the test author has nothing to go on.
        assert!(text.contains("exhausted"), "{text}");
        assert!(text.contains('1'), "{text}");
        assert!(text.contains('2'), "{text}");
    }

    #[tokio::test]
    async fn tool_call_step_reports_tool_use() {
        let provider =
            MockProvider::new().respond_with_tool_call("web_search", json!({"query": "rust"}));
        let resp = provider.completion(&req("search")).await.unwrap();

        assert!(resp.tool_use());
        let calls = resp.tool_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "web_search");
        assert_eq!(calls[0].input, json!({"query": "rust"}));
        assert_eq!(calls[0].id, "call_1");
    }

    #[tokio::test]
    async fn generated_call_ids_are_distinct() {
        // Two calls sharing an id would make a parallel-dispatch test pass for
        // the wrong reason.
        let provider = MockProvider::new()
            .respond_with_tool_call("a", json!({}))
            .respond_with_tool_call("b", json!({}));
        let first = provider.completion(&req("x")).await.unwrap();
        let second = provider.completion(&req("y")).await.unwrap();
        assert_ne!(first.tool_calls()[0].id, second.tool_calls()[0].id);
    }

    #[tokio::test]
    async fn requests_are_captured_with_their_parameters() {
        let provider = MockProvider::new().respond("ok");
        let request = CompletionRequest::new("mock-model", vec![Message::user("hi")])
            .with_temperature(0.25)
            .with_max_tokens(64)
            .with_tools(vec![json!({"name": "t"})]);
        provider.completion(&request).await.unwrap();

        let seen = provider.requests();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].messages.len(), 1);
        assert_eq!(seen[0].temperature, Some(0.25));
        assert_eq!(seen[0].max_tokens, Some(64));
        assert!(seen[0].tools.is_some());
        assert_eq!(provider.last_request().unwrap().messages[0].content, "hi");
    }

    #[tokio::test]
    async fn failed_requests_are_still_captured() {
        // A test asserting what was sent before a failure needs this.
        let provider = MockProvider::new().fail_with(|| CosmosError::Server {
            provider: PROVIDER_NAME.into(),
            status: 500,
            message: "boom".into(),
        });
        provider.completion(&req("hi")).await.unwrap_err();
        assert_eq!(provider.requests().len(), 1);
    }

    #[tokio::test]
    async fn fail_with_produces_the_exact_variant() {
        let provider = MockProvider::new().fail_with(|| CosmosError::RateLimit {
            provider: PROVIDER_NAME.into(),
            message: "slow down".into(),
            retry_after: Some(Duration::from_secs(20)),
        });

        let err = provider.completion(&req("hi")).await.unwrap_err();
        assert!(matches!(err, CosmosError::RateLimit { .. }));
        assert_eq!(err.retry_after(), Some(Duration::from_secs(20)));
        assert!(err.is_retryable());
    }

    #[tokio::test]
    async fn fail_times_then_respond_scripts_a_retry() {
        let provider = MockProvider::new()
            .fail_times(2, || CosmosError::Server {
                provider: PROVIDER_NAME.into(),
                status: 503,
                message: "overloaded".into(),
            })
            .respond("ok");

        assert!(provider.completion(&req("a")).await.is_err());
        assert!(provider.completion(&req("a")).await.is_err());
        assert_eq!(provider.completion(&req("a")).await.unwrap().text(), "ok");
        assert_eq!(provider.remaining(), 0);
    }

    #[tokio::test]
    async fn every_error_class_round_trips_with_its_retryability() {
        // The plan's verification item: a mock returning each class, asserting
        // is_retryable per class.
        let cases: Vec<(fn() -> CosmosError, bool)> = vec![
            (
                || CosmosError::RateLimit {
                    provider: PROVIDER_NAME.into(),
                    message: "m".into(),
                    retry_after: None,
                },
                true,
            ),
            (
                || CosmosError::Server {
                    provider: PROVIDER_NAME.into(),
                    status: 500,
                    message: "m".into(),
                },
                true,
            ),
            (
                || CosmosError::Timeout {
                    provider: PROVIDER_NAME.into(),
                    elapsed: Duration::from_secs(1),
                },
                true,
            ),
            (
                || CosmosError::Authentication {
                    provider: PROVIDER_NAME.into(),
                    message: "m".into(),
                },
                false,
            ),
            (
                || CosmosError::InsufficientQuota {
                    provider: PROVIDER_NAME.into(),
                    message: "m".into(),
                },
                false,
            ),
            (
                || CosmosError::ContextLength {
                    provider: PROVIDER_NAME.into(),
                    limit: Some(128_000),
                },
                false,
            ),
            (
                || CosmosError::ContentFiltered {
                    provider: PROVIDER_NAME.into(),
                    reason: "m".into(),
                },
                false,
            ),
            (
                || CosmosError::ModelNotFound {
                    provider: PROVIDER_NAME.into(),
                    model: "m".into(),
                },
                false,
            ),
        ];

        for (build, retryable) in cases {
            let provider = MockProvider::new().fail_with(build);
            let err = provider.completion(&req("hi")).await.unwrap_err();
            assert_eq!(err.is_retryable(), retryable, "{err}");
            assert_eq!(err.provider(), PROVIDER_NAME);
        }
    }

    #[tokio::test]
    async fn canned_echo_returns_the_last_user_message() {
        let provider = MockProvider::new().respond_canned(Canned::Echo);
        let request = CompletionRequest::new(
            "mock-model",
            vec![
                Message::system("ignored"),
                Message::user("first"),
                Message::assistant("reply"),
                Message::user("most recent"),
            ],
        );
        assert_eq!(
            provider.completion(&request).await.unwrap().text(),
            "most recent"
        );
    }

    #[tokio::test]
    async fn canned_count_and_fixed() {
        let provider = MockProvider::new()
            .respond_canned(Canned::Count)
            .respond_canned(Canned::Fixed("always".into()));

        let request = CompletionRequest::new(
            "mock-model",
            vec![Message::user("a"), Message::assistant("b")],
        );
        assert_eq!(provider.completion(&request).await.unwrap().text(), "2");
        assert_eq!(
            provider.completion(&request).await.unwrap().text(),
            "always"
        );
    }

    #[tokio::test]
    async fn usage_scales_with_response_length() {
        let provider = MockProvider::new()
            .respond("short")
            .respond("a considerably longer response than the previous one");

        let brief = provider.completion(&req("hi")).await.unwrap();
        let long = provider.completion(&req("hi")).await.unwrap();

        let brief_tokens = brief.usage.unwrap().completion_tokens;
        let long_tokens = long.usage.unwrap().completion_tokens;
        assert!(brief_tokens > 0);
        assert!(
            long_tokens > brief_tokens,
            "{long_tokens} vs {brief_tokens}"
        );
    }

    #[tokio::test]
    async fn canned_usage_counts_the_prompt() {
        let provider = MockProvider::new().respond_canned(Canned::Echo);
        let request = CompletionRequest::new(
            "mock-model",
            vec![Message::user("a prompt long enough to count for something")],
        );
        let usage = provider.completion(&request).await.unwrap().usage.unwrap();
        assert!(usage.prompt_tokens > 0);
        assert_eq!(
            usage.total_tokens,
            usage.prompt_tokens + usage.completion_tokens
        );
    }

    #[tokio::test]
    async fn streaming_matches_non_streaming_for_every_chunking() {
        // The property the harness's streaming tests rest on. If it does not
        // hold here, those tests are theatre.
        for chunking in [
            Chunking::Whole,
            Chunking::PerCharacter,
            Chunking::PerWord,
            Chunking::Fixed(3),
            Chunking::Fixed(1),
            Chunking::Fixed(0),
        ] {
            let text = "Hello there, general Kenobi.";

            let direct = MockProvider::new().respond(text);
            let expected = direct.completion(&req("hi")).await.unwrap();

            let streamed = MockProvider::new()
                .with_chunking(chunking.clone())
                .respond(text);
            let actual = accumulate(&streamed, &req("hi")).await;

            assert_eq!(actual.text(), expected.text(), "{chunking:?}");
            assert_eq!(
                actual.choices[0].finish_reason, expected.choices[0].finish_reason,
                "{chunking:?}"
            );
            assert_eq!(actual.usage, expected.usage, "{chunking:?}");
        }
    }

    #[tokio::test]
    async fn tool_arguments_split_mid_token_reassemble() {
        // Per-character chunking cuts `{"city":"Boston"}` between every pair of
        // characters. This is where real accumulators break.
        for chunking in [
            Chunking::PerCharacter,
            Chunking::Fixed(2),
            Chunking::Fixed(7),
        ] {
            let provider = MockProvider::new()
                .with_chunking(chunking.clone())
                .respond_with_tool_call("get_weather", json!({"city": "Boston", "unit": "f"}));

            let resp = accumulate(&provider, &req("weather")).await;
            let calls = resp.tool_calls();

            assert_eq!(calls.len(), 1, "{chunking:?}");
            assert_eq!(calls[0].name, "get_weather", "{chunking:?}");
            assert_eq!(
                calls[0].input,
                json!({"city": "Boston", "unit": "f"}),
                "{chunking:?}"
            );
        }
    }

    #[tokio::test]
    async fn streamed_parallel_tool_calls_stay_distinct() {
        let provider = MockProvider::new()
            .with_chunking(Chunking::PerCharacter)
            .respond_with_tool_calls(vec![
                ToolCall {
                    id: "a".into(),
                    name: "search".into(),
                    input: json!({"q": "rust"}),
                },
                ToolCall {
                    id: "b".into(),
                    name: "fetch".into(),
                    input: json!({"url": "http://x"}),
                },
            ]);

        let resp = accumulate(&provider, &req("both")).await;
        let calls = resp.tool_calls();

        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].id, "a");
        assert_eq!(calls[0].input, json!({"q": "rust"}));
        assert_eq!(calls[1].id, "b");
        assert_eq!(calls[1].input, json!({"url": "http://x"}));
    }

    #[tokio::test]
    async fn streamed_text_and_tool_calls_both_survive() {
        let provider = MockProvider::new()
            .with_chunking(Chunking::PerWord)
            .respond_with_text_and_tool_calls(
                "Let me look that up.",
                vec![ToolCall {
                    id: "call_1".into(),
                    name: "search".into(),
                    input: json!({"q": "x"}),
                }],
            );

        let resp = accumulate(&provider, &req("q")).await;
        assert_eq!(resp.text(), "Let me look that up.");
        assert_eq!(resp.tool_calls().len(), 1);
    }

    #[tokio::test]
    async fn fail_mid_stream_yields_chunks_then_the_error() {
        let provider = MockProvider::new()
            .with_chunking(Chunking::PerWord)
            .fail_mid_stream_after("one two three four", 2, || CosmosError::Streaming {
                provider: PROVIDER_NAME.into(),
                message: "connection reset".into(),
            });

        let mut stream = provider.stream_completion(&req("go")).await.unwrap();
        let mut seen = 0;
        let mut error = None;
        while let Some(item) = stream.next().await {
            match item {
                Ok(_) => seen += 1,
                Err(e) => {
                    error = Some(e);
                    break;
                }
            }
        }

        assert_eq!(seen, 2);
        let error = error.expect("stream should have failed");
        assert!(matches!(error, CosmosError::Streaming { .. }));
    }

    #[tokio::test]
    async fn mid_stream_failure_on_the_non_streaming_path_returns_the_error() {
        let provider =
            MockProvider::new().fail_mid_stream_after("text", 1, || CosmosError::Streaming {
                provider: PROVIDER_NAME.into(),
                message: "reset".into(),
            });
        assert!(provider.completion(&req("hi")).await.is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn delay_is_applied_before_responding() {
        let provider = MockProvider::new()
            .with_delay(Duration::from_millis(500))
            .respond("slow");

        let start = tokio::time::Instant::now();
        provider.completion(&req("hi")).await.unwrap();
        // Paused-clock time: the sleep is auto-advanced, so this asserts the
        // sleep happened rather than measuring wall time.
        assert!(start.elapsed() >= Duration::from_millis(500));
    }

    #[tokio::test]
    async fn models_are_configurable() {
        let provider = MockProvider::new().with_models(vec!["m1".into(), "m2".into()]);
        assert_eq!(provider.models().await.unwrap(), vec!["m1", "m2"]);
        assert!(provider.supports_streaming());
        assert_eq!(provider.name(), PROVIDER_NAME);
    }

    #[tokio::test]
    async fn respond_with_passes_a_response_through_verbatim() {
        let provider = MockProvider::new().respond_with(CompletionResponse {
            id: Some("resp_1".into()),
            model: Some("custom".into()),
            choices: vec![Choice {
                index: 0,
                message: Message::assistant("truncated"),
                finish_reason: Some("length".into()),
                tool_calls: vec![],
            }],
            usage: None,
        });

        let resp = provider.completion(&req("hi")).await.unwrap();
        assert_eq!(resp.id.as_deref(), Some("resp_1"));
        assert_eq!(resp.model.as_deref(), Some("custom"));
        assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("length"));
    }

    #[test]
    fn chunking_fragments_always_rejoin_to_the_input() {
        // Everything above depends on this; a strategy that drops or duplicates
        // a character would make the equivalence tests lie.
        let text = "héllo wörld — a mix of ascii and multi-byte";
        for chunking in [
            Chunking::Whole,
            Chunking::PerCharacter,
            Chunking::PerWord,
            Chunking::Fixed(1),
            Chunking::Fixed(5),
        ] {
            let joined: String = chunking.split(text).concat();
            assert_eq!(joined, text, "{chunking:?}");
        }
    }

    #[test]
    fn chunking_splits_empty_text_into_nothing() {
        for chunking in [Chunking::Whole, Chunking::PerCharacter, Chunking::PerWord] {
            assert!(chunking.split("").is_empty(), "{chunking:?}");
        }
    }

    #[test]
    fn per_word_chunking_produces_one_fragment_per_word() {
        assert_eq!(
            Chunking::PerWord.split("one two three"),
            vec!["one", " two", " three"]
        );
    }

    #[tokio::test]
    async fn works_through_the_client() {
        // The mock has to be usable wherever a real provider is, or consumers
        // cannot test the code paths that matter.
        let client = crate::Client::from_provider(Box::new(
            MockProvider::new().respond_canned(Canned::Echo),
        ))
        .with_model("mock-model");

        assert_eq!(client.complete("round trip").await.unwrap(), "round trip");
    }
}
