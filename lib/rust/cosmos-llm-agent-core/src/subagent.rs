//! Subagents (240): the `task` tool and the agents it can start.
//!
//! A child agent runs a fresh [`AgentLoop`] with its own history and returns
//! only its final text. The parent's context keeps the conclusion, not the file
//! dumps behind it.
//!
//! # Definitions
//!
//! [`AgentSpec`] is the IR. It holds names, never handles: tool names resolve
//! against the parent's registry when the child starts. Files are read from
//! `<config>/agents/` (user) then `<repo>/.cosmos/agents/` (project, wins):
//!
//! - `.trongo`: a single map, a list of maps, or `{:agents [...]}`, so one file
//!   can generate a family of agents with `defn` and `map`.
//! - `.md`: frontmatter (`name`, `description`, `tools`, `model`, `max-steps`)
//!   and the body as the prompt, as Claude Code writes them.
//!
//! Built in: `general` (the parent's tools) and `explore` (read-only search).
//!
//! # What a child may do
//!
//! - **Tools.** A subset of the parent's registry, by name (case-insensitive).
//!   `task` itself only while the depth limit allows (default 1: children
//!   cannot start children).
//! - **Policy.** A child cannot exceed its parent. Every call goes through the
//!   parent's approver chain: the same engine, rules, grants and prompts, so
//!   the user sees one approval stream. The approval's `call_id` is
//!   `<task call id>/<child call id>`, which routes the answer. A spec's
//!   `mode`, if stricter than the parent's, is applied first: a child in
//!   `read_only` is denied what read-only denies even if the parent could do it,
//!   and anything the stricter mode would ask about goes to the user even if the
//!   parent would have allowed it.
//! - **Hooks.** The child's tools are the parent's wrapped tools, so tool hooks
//!   run for them too.
//! - **Usage.** Rolls up into [`Event::RunFinished`](crate::Event) of the run
//!   that started the child.
//! - **Events.** Forwarded as [`Event::Subagent`](crate::Event), not mixed into
//!   the parent's stream. Child approval prompts arrive as ordinary
//!   `ApprovalRequested` events.
//! - **Cancellation.** The child's token is a child of the tool call's, so an
//!   interrupt reaches it.
//! - **Transcript.** A separate session file under the parent's session
//!   directory, linked from the tool result's `display`.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use cosmos_llm::{CompletionRequest, CompletionResponse, CosmosError, Message};
use cosmos_llm_tool::{
    AgentEvent, AgentLoop, ApprovalRequest, Approver, Completer, Decision, ParameterType, Registry,
    RunHandles, StopReason, TokenUsage, ToolContext, ToolDefinition,
};
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::config::{Layer, LoadOptions};
use crate::defs::{eval_def, listify, parse_frontmatter, read_text, walk_defs, DefKind};
use crate::error::{CoreError, Result};
use crate::eval::{ConfigEvaluator, EvalSnapshot};
use crate::policy::{ApprovalMode, PolicyEngine, PolicyVerdict};
use crate::protocol::Event;
use crate::session::SessionStore;

/// Separates a `task` call's id from the child's own call id in approval
/// requests (`toolu_1/toolu_9`).
pub const SUBAGENT_ID_SEP: char = '/';

/// A subagent definition: the IR `.trongo` and markdown files lower into.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct AgentSpec {
    /// The name the parent passes as `agent`.
    pub name: String,
    /// What it is for; listed in the `task` tool's description.
    pub description: String,
    /// Tool names to give it. Absent means all of the parent's tools except
    /// `task`.
    pub tools: Option<Vec<String>>,
    /// Model for the child. Absent means the parent's current model.
    pub model: Option<String>,
    /// Model turns allowed. Absent means the core's `max_steps`.
    #[serde(alias = "max_turns")]
    pub max_steps: Option<u32>,
    /// The system prompt.
    pub prompt: String,
    /// A mode to hold the child to if it is stricter than the parent's.
    pub mode: Option<ApprovalMode>,
    /// Which layer defined it; absent for the built-ins.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer: Option<Layer>,
}

impl AgentSpec {
    /// An agent with a name and prompt.
    pub fn new(name: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            prompt: prompt.into(),
            ..Self::default()
        }
    }

    /// Lowers one evaluated map.
    pub fn from_value(value: Value) -> Result<Self> {
        let Value::Object(mut map) = value else {
            return Err(CoreError::Config("an agent must be a map".into()));
        };
        listify(&mut map, "tools");
        serde_json::from_value(Value::Object(map))
            .map_err(|e| CoreError::Config(format!("value does not match AgentSpec: {e}")))
    }

    /// Lowers a markdown agent file.
    pub fn from_markdown(name: &str, text: &str) -> Result<Self> {
        let (mut map, body) = parse_frontmatter(text);
        map.insert("prompt".into(), Value::String(body));
        let mut spec = Self::from_value(Value::Object(map))?;
        if spec.name.is_empty() {
            spec.name = name.to_owned();
        }
        Ok(spec)
    }

    /// Lowers a `.trongo` file's value: one map, a list of maps, or
    /// `{:agents [...]}`. `fallback_name` names a single unnamed map.
    pub fn from_trongo_value(value: Value, fallback_name: &str) -> Result<Vec<Self>> {
        let items = match value {
            Value::Array(a) => a,
            Value::Object(mut m) if m.contains_key("agents") => match m.remove("agents") {
                Some(Value::Array(a)) => a,
                _ => return Err(CoreError::Config(":agents must be a list".into())),
            },
            other @ Value::Object(_) => vec![other],
            _ => {
                return Err(CoreError::Config(
                    "an agent file must evaluate to a map".into(),
                ))
            }
        };
        let single = items.len() == 1;
        items
            .into_iter()
            .map(|v| {
                let mut spec = Self::from_value(v)?;
                if spec.name.is_empty() {
                    if single {
                        spec.name = fallback_name.to_owned();
                    } else {
                        return Err(CoreError::Config(
                            "every agent in a list needs a name".into(),
                        ));
                    }
                }
                Ok(spec)
            })
            .collect()
    }

    /// The built-in `general` agent.
    pub fn general() -> Self {
        Self {
            name: "general".into(),
            description: "General-purpose agent with all of your tools. Use it for multi-step research or changes whose details you do not need in your own context.".into(),
            prompt: "You are a subagent working for another agent on a task it delegated. You see only the brief you were given. Work on it independently with your tools, then end with a final message that fully answers the brief: the other agent sees only that message, not your tool calls.".into(),
            ..Self::default()
        }
    }

    /// The built-in read-only `explore` agent.
    pub fn explore() -> Self {
        Self {
            name: "explore".into(),
            description: "Read-only codebase search. Use it to find where something is done or how a thing works; it cannot edit or run commands.".into(),
            tools: Some(["read", "glob", "grep", "list"].map(String::from).to_vec()),
            mode: Some(ApprovalMode::ReadOnly),
            prompt: "You are a read-only search subagent working for another agent. Find what the brief asks for by searching and reading files. You cannot change anything. End with a final message that answers the brief: name the files and lines that matter and quote only what is needed. The other agent sees only that message.".into(),
            ..Self::default()
        }
    }
}

/// The agents a core offers, by name.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentSet {
    specs: BTreeMap<String, AgentSpec>,
}

impl Default for AgentSet {
    fn default() -> Self {
        let mut s = Self {
            specs: BTreeMap::new(),
        };
        s.insert(AgentSpec::general());
        s.insert(AgentSpec::explore());
        s
    }
}

impl AgentSet {
    /// The built-ins only.
    pub fn new() -> Self {
        Self::default()
    }

    /// The built-ins, then `<user>/agents`, then `<project>/agents`.
    pub fn load(opts: &LoadOptions) -> (Self, Vec<String>) {
        let mut set = Self::new();
        let mut warnings = Vec::new();
        let snapshot = opts
            .snapshot
            .clone()
            .unwrap_or_else(|| EvalSnapshot::capture(&opts.cwd));
        let ev = ConfigEvaluator::new(snapshot);
        if let Some(u) = opts.resolved_user_dir() {
            set.load_dir(&u.join("agents"), Layer::User, &ev, &mut warnings);
        }
        set.load_dir(
            &opts.resolved_project_dir().join("agents"),
            Layer::Project,
            &ev,
            &mut warnings,
        );
        (set, warnings)
    }

    /// Adds every agent file under `dir`.
    pub fn load_dir(
        &mut self,
        dir: &Path,
        layer: Layer,
        ev: &ConfigEvaluator,
        warnings: &mut Vec<String>,
    ) {
        for file in walk_defs(dir) {
            let specs = match file.kind {
                DefKind::Trongo => eval_def(ev, &file.path)
                    .and_then(|v| AgentSpec::from_trongo_value(v, &file.name)),
                DefKind::Markdown => read_text(&file.path)
                    .and_then(|t| AgentSpec::from_markdown(&file.name, &t))
                    .map(|s| vec![s]),
            };
            match specs {
                Ok(list) => {
                    for mut spec in list {
                        if spec.prompt.trim().is_empty() {
                            warnings.push(format!(
                                "{}: agent `{}` has no prompt, skipped",
                                file.path.display(),
                                spec.name
                            ));
                            continue;
                        }
                        spec.layer = Some(layer);
                        self.insert(spec);
                    }
                }
                Err(e) => warnings.push(format!("{}: {e}", file.path.display())),
            }
        }
    }

    /// Adds an agent, replacing one of the same name.
    pub fn insert(&mut self, spec: AgentSpec) {
        self.specs.insert(spec.name.clone(), spec);
    }

    /// An agent by name.
    pub fn get(&self, name: &str) -> Option<&AgentSpec> {
        self.specs.get(name)
    }

    /// All agents, sorted by name.
    pub fn list(&self) -> Vec<AgentSpec> {
        self.specs.values().cloned().collect()
    }
}

/// Limits on subagents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SubagentOptions {
    /// How deep children may nest. 1 (the default): children get no `task`
    /// tool. 0 turns subagents off.
    pub max_depth: usize,
}

impl Default for SubagentOptions {
    fn default() -> Self {
        Self { max_depth: 1 }
    }
}

impl SubagentOptions {
    /// Options with the given depth limit.
    pub fn depth(max_depth: usize) -> Self {
        Self { max_depth }
    }
}

/// Fixes the model on every request, beneath any live settings.
struct ModelOverride {
    inner: Arc<dyn Completer>,
    model: String,
}

impl Completer for ModelOverride {
    fn completion<'a>(
        &'a self,
        req: &'a CompletionRequest,
    ) -> BoxFuture<'a, std::result::Result<CompletionResponse, CosmosError>> {
        let mut r = req.clone();
        r.model = self.model.clone();
        Box::pin(async move { self.inner.completion(&r).await })
    }

    fn stream<'a>(
        &'a self,
        req: &'a CompletionRequest,
    ) -> BoxFuture<'a, std::result::Result<cosmos_llm::CompletionStream, CosmosError>> {
        let mut r = req.clone();
        r.model = self.model.clone();
        Box::pin(async move { self.inner.stream(&r).await })
    }
}

/// The child's approver: labels the request, holds the child to a stricter
/// mode if its spec asks for one, then defers to the parent's chain.
struct ChildApprover {
    key: String,
    chain: Arc<dyn Approver>,
    interactive: Arc<dyn Approver>,
    engine: Arc<PolicyEngine>,
    mode: Option<ApprovalMode>,
}

impl Approver for ChildApprover {
    fn check<'a>(&'a self, req: &'a ApprovalRequest) -> BoxFuture<'a, Decision> {
        Box::pin(async move {
            let mut r = req.clone();
            r.call_id = format!("{}{SUBAGENT_ID_SEP}{}", self.key, req.call_id);
            let stricter = self
                .mode
                .filter(|m| m.permissiveness() < self.engine.mode().permissiveness());
            if let Some(mode) = stricter {
                match self.engine.derive(mode).evaluate(&r) {
                    PolicyVerdict::Deny { reason } => return Decision::deny(reason),
                    PolicyVerdict::Ask { .. } => {
                        // The parent may allow it outright; the stricter mode
                        // still wants the user's say, unless the parent denies.
                        if let PolicyVerdict::Deny { reason } = self.engine.evaluate(&r) {
                            return Decision::deny(reason);
                        }
                        let answer = self.interactive.check(&r).await;
                        return self.engine.observe(&r, answer).0;
                    }
                    _ => {}
                }
            }
            self.chain.check(&r).await
        })
    }
}

/// Everything the `task` tool needs from the core. One per nesting level;
/// children get their own with `depth + 1`.
pub(crate) struct SubagentHost {
    pub agents: Arc<AgentSet>,
    /// The parent's tools (hook-wrapped), without `task`.
    pub base: Arc<Registry>,
    /// Applies the parent's live model and reasoning.
    pub parent_completer: Arc<dyn Completer>,
    /// The bare client, for children that name their own model.
    pub raw_completer: Arc<dyn Completer>,
    pub events: mpsc::Sender<Event>,
    pub chain: Arc<dyn Approver>,
    pub interactive: Arc<dyn Approver>,
    pub engine: Arc<PolicyEngine>,
    pub depth: usize,
    pub max_depth: usize,
    /// `""` at the top, `"<key>/"` below a child.
    pub prefix: String,
    /// Shared by the whole tree.
    pub usage: Arc<Mutex<TokenUsage>>,
    /// `<parent session dir>/subagents`, when the parent logs.
    pub log_root: Option<PathBuf>,
    pub cwd: PathBuf,
    pub default_max_steps: u32,
}

impl SubagentHost {
    fn child(&self, key: &str) -> SubagentHost {
        SubagentHost {
            agents: Arc::clone(&self.agents),
            base: Arc::clone(&self.base),
            parent_completer: Arc::clone(&self.parent_completer),
            raw_completer: Arc::clone(&self.raw_completer),
            events: self.events.clone(),
            chain: Arc::clone(&self.chain),
            interactive: Arc::clone(&self.interactive),
            engine: Arc::clone(&self.engine),
            depth: self.depth + 1,
            max_depth: self.max_depth,
            prefix: format!("{key}{SUBAGENT_ID_SEP}"),
            usage: Arc::clone(&self.usage),
            log_root: self.log_root.clone(),
            cwd: self.cwd.clone(),
            default_max_steps: self.default_max_steps,
        }
    }

    /// Takes the usage children have reported since the last call.
    pub fn take_usage(&self) -> TokenUsage {
        std::mem::take(&mut *self.usage.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// The `task` tool.
    pub fn task_tool(self: &Arc<Self>) -> ToolDefinition {
        let host = Arc::clone(self);
        let mut listing = String::new();
        for a in self.agents.list() {
            listing.push_str(&format!("\n- {}: {}", a.name, a.description));
        }
        ToolDefinition::new("task")
            .description(format!(
                "Run a subagent on a self-contained task and get back only its final answer. \
                 The subagent starts with no context: put everything it needs in `prompt`. \
                 Several `task` calls in one turn run in parallel.\n\nAgents:{listing}"
            ))
            .param(
                "description",
                ParameterType::String,
                true,
                "A short label (3-5 words) for the UI.",
            )
            .param(
                "prompt",
                ParameterType::String,
                true,
                "The full brief. The subagent sees nothing else.",
            )
            .param(
                "agent",
                ParameterType::String,
                false,
                "Which agent to run. Defaults to `general`.",
            )
            // Starting a child is harmless; each of its own calls is checked.
            .read_only(true)
            .context_handler(move |params, ctx| {
                let host = Arc::clone(&host);
                Box::pin(async move { host.run(params, ctx).await })
            })
    }

    async fn run(
        self: Arc<Self>,
        params: Value,
        ctx: ToolContext,
    ) -> std::result::Result<Value, String> {
        let get = |k: &str| params.get(k).and_then(Value::as_str).unwrap_or("");
        let prompt = get("prompt").trim().to_owned();
        if prompt.is_empty() {
            return Err("`prompt` is empty; the subagent sees nothing else".into());
        }
        let description = get("description").to_owned();
        let name = if get("agent").is_empty() {
            "general"
        } else {
            get("agent")
        };
        let spec = self.agents.get(name).cloned().ok_or_else(|| {
            let names: Vec<String> = self.agents.list().into_iter().map(|a| a.name).collect();
            format!("unknown agent `{name}`. Available: {}", names.join(", "))
        })?;
        if self.depth >= self.max_depth {
            return Err("subagents are not available at this depth".into());
        }
        let key = format!("{}{}", self.prefix, ctx.call_id);

        // Tools: a subset of the parent's, by name.
        let allowed: Option<HashSet<String>> = spec
            .tools
            .as_ref()
            .map(|t| t.iter().map(|n| n.to_ascii_lowercase()).collect());
        let mut registry = Registry::new();
        for tool in self.base.all() {
            if tool.name == "task" {
                continue;
            }
            if allowed
                .as_ref()
                .map_or(true, |a| a.contains(&tool.name.to_ascii_lowercase()))
            {
                registry.register(tool.clone());
            }
        }
        if self.depth + 1 < self.max_depth && allowed.as_ref().map_or(true, |a| a.contains("task"))
        {
            registry.register(Arc::new(self.child(&key)).task_tool());
        }

        let approver = Arc::new(ChildApprover {
            key: key.clone(),
            chain: Arc::clone(&self.chain),
            interactive: Arc::clone(&self.interactive),
            engine: Arc::clone(&self.engine),
            mode: spec.mode,
        });
        let system = format!(
            "{}\n\nWorking directory: {}",
            spec.prompt.trim_end(),
            self.cwd.display()
        );
        let agent = AgentLoop::new(registry)
            .with_system(system)
            .with_max_steps(spec.max_steps.unwrap_or(self.default_max_steps))
            .with_approver(approver);
        let completer: Arc<dyn Completer> = match &spec.model {
            Some(m) => Arc::new(ModelOverride {
                inner: Arc::clone(&self.raw_completer),
                model: m.clone(),
            }),
            None => Arc::clone(&self.parent_completer),
        };

        // Events: forwarded wrapped, approvals left to the parent's stream.
        let (tx, mut rx) = cosmos_llm_tool::event_channel(256);
        let forward = {
            let out = self.events.clone();
            let call_id = key.clone();
            let agent_name = spec.name.clone();
            tokio::spawn(async move {
                while let Some(ev) = rx.recv().await {
                    if matches!(
                        ev,
                        AgentEvent::ApprovalRequested { .. } | AgentEvent::ApprovalResolved { .. }
                    ) {
                        continue;
                    }
                    let _ = out
                        .send(Event::Subagent {
                            call_id: call_id.clone(),
                            agent: agent_name.clone(),
                            inner: Box::new(ev),
                        })
                        .await;
                }
            })
        };

        let mut history = vec![Message::user(prompt)];
        let handles = RunHandles::new()
            .events(tx)
            .cancel(ctx.cancel.child_token());
        let result = agent
            .run_with(completer.as_ref(), &mut history, &handles)
            .await;
        drop(handles);
        let _ = forward.await;

        let transcript = self.save_transcript(&spec, &history).await;
        match result {
            Ok(out) => {
                self.usage
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .add(&out.usage);
                ctx.set_display(json!({
                    "type": "subagent",
                    "agent": spec.name,
                    "description": description,
                    "steps": out.steps_taken,
                    "usage": out.usage,
                    "session_path": transcript,
                }));
                match out.reason {
                    StopReason::Interrupted => Err("the subagent was interrupted".into()),
                    _ if out.text.trim().is_empty() => Ok(json!(format!(
                        "(the subagent stopped after {} step(s) without a final answer)",
                        out.steps_taken
                    ))),
                    _ => Ok(Value::String(out.text)),
                }
            }
            Err(e) => Err(format!("the subagent failed: {e}")),
        }
    }

    async fn save_transcript(&self, spec: &AgentSpec, history: &[Message]) -> Option<PathBuf> {
        let root = self.log_root.as_ref()?;
        let store = SessionStore::new(root);
        let w = store
            .create(&self.cwd, spec.model.as_deref().unwrap_or("inherited"))
            .ok()?;
        for (i, m) in history.iter().enumerate() {
            w.append_message(i as u64 + 1, m, None).ok()?;
        }
        let path = w.path().to_owned();
        let _ = w.close().await;
        Some(path)
    }
}
