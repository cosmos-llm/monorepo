//! Hooks (230): the user's own commands around the agent.
//!
//! | Event | When | Can |
//! |---|---|---|
//! | `pre_tool_use` | after approval, before the tool runs | block with a reason, rewrite the input |
//! | `post_tool_use` | after the tool ran | append context to the result the model sees |
//! | `user_prompt_submit` | before input reaches the model | block, add context |
//! | `stop` | a run ends | block, which asks the agent to continue |
//! | `session_start` | the core starts | add context |
//!
//! A hook is a shell command ([`HookSpec`]). It gets a [`HookPayload`] on
//! stdin, as JSON (a superset of Claude Code's fields, so existing scripts
//! work unchanged) or, with `:payload-format :trongo`, as TrongoScript data.
//! It answers with an exit code (0 ok, 2 block with stderr as the reason,
//! anything else a non-blocking error shown as a warning) and optional JSON on
//! stdout ([`HookResponse`]).
//!
//! # Where they wrap tool calls
//!
//! `pre_tool_use` and `post_tool_use` wrap the tool's own handler
//! ([`HookRunner::wrap_registry`]), so they run after the approver chain
//! (a denied call never reaches a hook) and inside the tool's timeout, and
//! subagents built from the wrapped registry inherit them. No change to the
//! loop is needed. A rewritten input is not re-validated against the tool's
//! schema.
//!
//! # Trust
//!
//! Hooks run arbitrary commands. Hooks from the user's config always run.
//! Hooks from a repository's `.cosmos` directory run only when the
//! [`LoadOptions::trust_project`](crate::LoadOptions) flag is set or the
//! [`HookTrust`] store holds the hash of exactly those definitions;
//! otherwise [`LoadedHooks::pending`] carries them for the frontend to show,
//! and a warning is emitted.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cosmos_llm::CancellationToken;
use cosmos_llm_tool::{Handler, Registry, ToolContext};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::UnboundedSender;

use crate::config::LoadOptions;
use crate::defs::normalize_keys;
use crate::error::{CoreError, Result};
use crate::eval::{ConfigEvaluator, EvalSnapshot};
use crate::policy::shell::wildcard_match;

/// When a hook runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum HookEvent {
    /// After approval, before execution.
    #[serde(alias = "pre-tool-use", alias = "PreToolUse")]
    PreToolUse,
    /// After execution.
    #[serde(alias = "post-tool-use", alias = "PostToolUse")]
    PostToolUse,
    /// Before input reaches the model.
    #[serde(alias = "user-prompt-submit", alias = "UserPromptSubmit")]
    UserPromptSubmit,
    /// A run ended.
    #[serde(alias = "Stop")]
    Stop,
    /// The core started.
    #[serde(alias = "session-start", alias = "SessionStart")]
    SessionStart,
}

impl HookEvent {
    /// Every event.
    pub const ALL: [HookEvent; 5] = [
        Self::PreToolUse,
        Self::PostToolUse,
        Self::UserPromptSubmit,
        Self::Stop,
        Self::SessionStart,
    ];

    /// The name Claude Code scripts see in `hook_event_name`.
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::PreToolUse => "PreToolUse",
            Self::PostToolUse => "PostToolUse",
            Self::UserPromptSubmit => "UserPromptSubmit",
            Self::Stop => "Stop",
            Self::SessionStart => "SessionStart",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        let k: String = key
            .chars()
            .filter(|c| *c != '_' && *c != '-')
            .collect::<String>()
            .to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|e| e.wire_name().to_ascii_lowercase() == k)
    }

    fn is_tool_event(self) -> bool {
        matches!(self, Self::PreToolUse | Self::PostToolUse)
    }
}

/// How the payload is written to the hook's stdin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum PayloadFormat {
    /// JSON, compatible with Claude Code hook scripts.
    #[default]
    Json,
    /// TrongoScript data literals.
    Trongo,
}

fn default_timeout_ms() -> u64 {
    30_000
}

/// One hook: the IR a `:hooks` config lowers into.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct HookSpec {
    /// When it runs.
    pub event: HookEvent,
    /// For tool events, the tools it applies to: names joined by `|`, `*` as a
    /// wildcard. Empty or absent means every tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matcher: Option<String>,
    /// The shell command. Runs under `sh -c` in the workspace.
    pub command: String,
    /// Longest it may run before it is killed (a non-blocking error).
    pub timeout_ms: u64,
    /// How stdin is written.
    pub payload_format: PayloadFormat,
}

impl Default for HookSpec {
    fn default() -> Self {
        Self {
            event: HookEvent::PostToolUse,
            matcher: None,
            command: String::new(),
            timeout_ms: default_timeout_ms(),
            payload_format: PayloadFormat::Json,
        }
    }
}

impl HookSpec {
    /// A hook for `event` running `command`.
    pub fn new(event: HookEvent, command: impl Into<String>) -> Self {
        Self {
            event,
            command: command.into(),
            ..Self::default()
        }
    }

    /// Restricts a tool-event hook to matching tool names.
    #[must_use]
    pub fn matcher(mut self, matcher: impl Into<String>) -> Self {
        self.matcher = Some(matcher.into());
        self
    }

    /// Sets the timeout.
    #[must_use]
    pub fn timeout_ms(mut self, ms: u64) -> Self {
        self.timeout_ms = ms;
        self
    }

    /// Whether this hook applies to `tool`.
    pub fn matches_tool(&self, tool: &str) -> bool {
        let Some(m) = self.matcher.as_deref().map(str::trim) else {
            return true;
        };
        if m.is_empty() || m == "*" {
            return true;
        }
        let tool = tool.to_ascii_lowercase();
        m.split('|')
            .map(|p| p.trim().to_ascii_lowercase())
            .any(|p| wildcard_match(&p, &tool))
    }

    /// Lowers an evaluated `hooks` value into specs.
    ///
    /// Accepts a list of specs (each with an `event`), or a map keyed by event
    /// (`:post-tool-use`, `post_tool_use` or Claude Code's `PostToolUse`)
    /// whose values are lists of specs. A Claude Code style entry
    /// `{matcher, hooks: [{type: command, command, timeout}]}` is accepted too
    /// (its `timeout` is in seconds).
    pub fn from_config(value: Value) -> Result<Vec<HookSpec>> {
        let value = normalize_keys(value);
        let mut out = Vec::new();
        match value {
            Value::Null => {}
            Value::Array(items) => {
                for item in items {
                    out.push(parse_spec(item, None)?);
                }
            }
            Value::Object(map) => {
                for (key, entries) in map {
                    let event = HookEvent::from_key(&key).ok_or_else(|| {
                        CoreError::Config(format!("hooks: unknown event `{key}`"))
                    })?;
                    let Value::Array(entries) = entries else {
                        return Err(CoreError::Config(format!("hooks: `{key}` must be a list")));
                    };
                    for entry in entries {
                        if let Some(inner) = entry.get("hooks").and_then(Value::as_array) {
                            let matcher = entry.get("matcher").cloned();
                            for h in inner {
                                let mut h = h.clone();
                                if let (Some(m), Some(o)) = (&matcher, h.as_object_mut()) {
                                    o.entry("matcher").or_insert_with(|| m.clone());
                                }
                                if let Some(o) = h.as_object_mut() {
                                    if let Some(secs) = o.remove("timeout").and_then(|t| t.as_u64())
                                    {
                                        o.insert("timeout_ms".into(), json!(secs * 1000));
                                    }
                                }
                                out.push(parse_spec(h, Some(event))?);
                            }
                        } else {
                            out.push(parse_spec(entry, Some(event))?);
                        }
                    }
                }
            }
            _ => {
                return Err(CoreError::Config(
                    "hooks must be a list or a map of lists".into(),
                ))
            }
        }
        Ok(out)
    }
}

fn parse_spec(mut item: Value, event: Option<HookEvent>) -> Result<HookSpec> {
    if let (Some(e), Some(o)) = (event, item.as_object_mut()) {
        o.insert(
            "event".into(),
            serde_json::to_value(e).map_err(CoreError::from)?,
        );
        o.remove("type");
    }
    if let Some(o) = item.as_object_mut() {
        // `:command-type`-style leftovers from Claude Code entries.
        o.remove("type");
    }
    let spec: HookSpec = serde_json::from_value(item)
        .map_err(|e| CoreError::Config(format!("value does not match HookSpec: {e}")))?;
    if spec.command.trim().is_empty() {
        return Err(CoreError::Config("a hook needs a command".into()));
    }
    Ok(spec)
}

/// What a hook is told. The IR written to stdin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct HookPayload {
    /// Which event fired.
    pub event: HookEvent,
    /// The session id.
    pub session_id: String,
    /// The workspace.
    pub cwd: PathBuf,
    /// The tool, for tool events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// The tool's input, for tool events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    /// The tool's result, for `post_tool_use`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// The user's text, for `user_prompt_submit`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// The agent's last text, for `stop`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assistant_message: Option<String>,
    /// For `stop`: this run was itself continued by a stop hook.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub stop_hook_active: bool,
}

impl Default for HookPayload {
    fn default() -> Self {
        Self {
            event: HookEvent::SessionStart,
            session_id: String::new(),
            cwd: PathBuf::new(),
            tool: None,
            input: None,
            result: None,
            prompt: None,
            assistant_message: None,
            stop_hook_active: false,
        }
    }
}

impl HookPayload {
    /// A payload for `event` with nothing else set.
    pub fn new(event: HookEvent) -> Self {
        Self {
            event,
            ..Self::default()
        }
    }

    /// The JSON written to stdin: the IR plus the names Claude Code scripts
    /// read (`hook_event_name`, `tool_name`, `tool_input`, `tool_response`).
    pub fn to_wire(&self) -> Value {
        let mut v = serde_json::to_value(self).unwrap_or(Value::Null);
        if let Some(o) = v.as_object_mut() {
            o.insert("hook_event_name".into(), json!(self.event.wire_name()));
            if let Some(t) = &self.tool {
                o.insert("tool_name".into(), json!(t));
            }
            if let Some(i) = &self.input {
                o.insert("tool_input".into(), i.clone());
            }
            if let Some(r) = &self.result {
                o.insert("tool_response".into(), r.clone());
            }
        }
        v
    }

    /// The file paths a tool input names (`file_path`, `path`, `paths`).
    pub fn file_paths(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(i) = &self.input {
            for key in ["file_path", "path"] {
                if let Some(s) = i.get(key).and_then(Value::as_str) {
                    out.push(s.to_owned());
                }
            }
            if let Some(a) = i.get("paths").and_then(Value::as_array) {
                out.extend(a.iter().filter_map(Value::as_str).map(str::to_owned));
            }
        }
        out
    }
}

/// A hook's verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum HookDecision {
    /// Go ahead (Claude Code also says `approve`).
    #[serde(alias = "approve")]
    Allow,
    /// Stop it (also `deny`).
    #[serde(alias = "deny")]
    Block,
}

/// What a hook may print on stdout.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct HookResponse {
    /// Allow or block.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<HookDecision>,
    /// Why, shown to the model when blocking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Text added to the result (tool events) or the input (prompt and start).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub additional_context: Option<String>,
    /// A replacement tool input, for `pre_tool_use`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_input: Option<Value>,
}

impl HookResponse {
    /// Parses stdout as a response. Claude Code's camelCase names and the
    /// `hookSpecificOutput` wrapper are understood. `None` when stdout is not a
    /// JSON object.
    pub fn from_stdout(stdout: &str) -> Option<Self> {
        let v: Value = serde_json::from_str(stdout.trim()).ok()?;
        let mut obj = v.as_object()?.clone();
        if let Some(Value::Object(inner)) = obj.remove("hookSpecificOutput") {
            for (k, v) in inner {
                obj.entry(k).or_insert(v);
            }
        }
        let mut flat = serde_json::Map::new();
        for (k, v) in obj {
            let key = match k.as_str() {
                "additionalContext" => "additional_context".to_owned(),
                "updatedInput" => "updated_input".to_owned(),
                "permissionDecision" => "decision".to_owned(),
                "permissionDecisionReason" => "reason".to_owned(),
                other => other.replace('-', "_"),
            };
            flat.entry(key).or_insert(v);
        }
        serde_json::from_value(Value::Object(flat)).ok()
    }
}

/// What running every matching hook for one event came to.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct HookOutcome {
    /// Set when a hook blocked: its reason.
    pub block: Option<String>,
    /// Context hooks asked to add, in order.
    pub context: Vec<String>,
    /// The last rewritten tool input, if any.
    pub updated_input: Option<Value>,
    /// Non-blocking problems (a hook exited 1, timed out, would not start).
    pub warnings: Vec<String>,
    /// How many hooks ran.
    pub ran: usize,
}

/// Runs hooks. Cheap to share: one per core.
pub struct HookRunner {
    hooks: Vec<HookSpec>,
    cwd: PathBuf,
    session_id: Mutex<String>,
    warn: Option<UnboundedSender<String>>,
}

impl std::fmt::Debug for HookRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HookRunner")
            .field("hooks", &self.hooks.len())
            .finish_non_exhaustive()
    }
}

impl HookRunner {
    /// A runner for `hooks`, run in `cwd`.
    pub fn new(hooks: Vec<HookSpec>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            hooks,
            cwd: cwd.into(),
            session_id: Mutex::new(String::new()),
            warn: None,
        }
    }

    /// Sends non-blocking problems to `tx` as well as returning them.
    #[must_use]
    pub fn with_warnings(mut self, tx: UnboundedSender<String>) -> Self {
        self.warn = Some(tx);
        self
    }

    /// Sets the session id hooks are told.
    pub fn set_session_id(&self, id: &str) {
        *self.session_id.lock().unwrap_or_else(|e| e.into_inner()) = id.to_owned();
    }

    /// Whether any hook is registered for `event`.
    pub fn has(&self, event: HookEvent) -> bool {
        self.hooks.iter().any(|h| h.event == event)
    }

    /// Whether there are no hooks at all.
    pub fn is_empty(&self) -> bool {
        self.hooks.is_empty()
    }

    /// A payload for `event` carrying the session id and workspace.
    pub fn payload(&self, event: HookEvent) -> HookPayload {
        let mut p = HookPayload::new(event);
        p.session_id = self
            .session_id
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        p.cwd = self.cwd.clone();
        p
    }

    /// Runs the hooks matching `payload`, in config order, until one blocks.
    /// `cancel` kills a hook that is still running.
    pub async fn fire(&self, payload: &HookPayload, cancel: &CancellationToken) -> HookOutcome {
        let mut out = HookOutcome::default();
        let mut payload = payload.clone();
        for hook in &self.hooks {
            if hook.event != payload.event {
                continue;
            }
            if hook.event.is_tool_event()
                && !hook.matches_tool(payload.tool.as_deref().unwrap_or(""))
            {
                continue;
            }
            out.ran += 1;
            let run = run_hook(hook, &payload, &self.cwd, cancel).await;
            if let Some(w) = run.warning {
                if let Some(tx) = &self.warn {
                    let _ = tx.send(w.clone());
                }
                out.warnings.push(w);
            }
            if let Some(reason) = run.block {
                out.block = Some(reason);
                break;
            }
            if let Some(c) = run
                .response
                .additional_context
                .filter(|c| !c.trim().is_empty())
            {
                out.context.push(c);
            }
            if let Some(u) = run.response.updated_input {
                payload.input = Some(u.clone());
                out.updated_input = Some(u);
            }
        }
        out
    }

    /// Wraps every tool's handler in `registry` with the `pre_tool_use` and
    /// `post_tool_use` hooks. A registry is returned unchanged when no tool
    /// hook is registered.
    pub fn wrap_registry(self: &Arc<Self>, registry: Registry) -> Registry {
        if !self.has(HookEvent::PreToolUse) && !self.has(HookEvent::PostToolUse) {
            return registry;
        }
        let mut out = Registry::new();
        for tool in registry.all() {
            let Some(inner) = tool.handler_ref().cloned() else {
                out.register(tool.clone());
                continue;
            };
            let name = tool.name.clone();
            let runner = Arc::clone(self);
            out.register(tool.clone().context_handler(move |params, ctx| {
                let (inner, name, runner) = (inner.clone(), name.clone(), Arc::clone(&runner));
                Box::pin(async move { run_wrapped(&runner, &name, inner, params, ctx).await })
            }));
        }
        out
    }
}

async fn call_handler(h: Handler, params: Value, ctx: ToolContext) -> Result<Value, String> {
    match h {
        Handler::Sync(f) => tokio::task::spawn_blocking(move || f(&params))
            .await
            .unwrap_or_else(|e| Err(format!("handler panicked: {e}"))),
        Handler::Async(f) => f(params).await,
        Handler::Context(f) => f(params, ctx).await,
        _ => Err("unsupported handler kind".to_owned()),
    }
}

async fn run_wrapped(
    runner: &HookRunner,
    name: &str,
    inner: Handler,
    mut params: Value,
    ctx: ToolContext,
) -> Result<Value, String> {
    let mut extra: Vec<String> = Vec::new();
    if runner.has(HookEvent::PreToolUse) {
        let mut p = runner.payload(HookEvent::PreToolUse);
        p.tool = Some(name.to_owned());
        p.input = Some(params.clone());
        let out = runner.fire(&p, &ctx.cancel).await;
        if let Some(reason) = out.block {
            return Err(format!("blocked by a PreToolUse hook: {reason}"));
        }
        if let Some(u) = out.updated_input {
            params = u;
        }
        extra.extend(out.context);
    }
    let value = call_handler(inner, params.clone(), ctx.clone()).await?;
    if runner.has(HookEvent::PostToolUse) {
        let mut p = runner.payload(HookEvent::PostToolUse);
        p.tool = Some(name.to_owned());
        p.input = Some(params);
        p.result = Some(value.clone());
        let out = runner.fire(&p, &ctx.cancel).await;
        extra.extend(out.context);
        if let Some(reason) = out.block {
            extra.push(format!("A PostToolUse hook objected: {reason}"));
        }
    }
    if extra.is_empty() {
        return Ok(value);
    }
    let mut text = match value {
        Value::String(s) => s,
        other => other.to_string(),
    };
    for e in extra {
        text.push_str("\n\n[hook] ");
        text.push_str(e.trim());
    }
    Ok(Value::String(text))
}

struct HookRun {
    response: HookResponse,
    block: Option<String>,
    warning: Option<String>,
}

fn shell_hook_error(spec: &HookSpec, why: impl std::fmt::Display) -> HookRun {
    HookRun {
        response: HookResponse::default(),
        block: None,
        warning: Some(format!(
            "{} hook `{}`: {why}",
            spec.event.wire_name(),
            spec.command
        )),
    }
}

async fn run_hook(
    spec: &HookSpec,
    payload: &HookPayload,
    cwd: &Path,
    cancel: &CancellationToken,
) -> HookRun {
    let stdin_text = match spec.payload_format {
        PayloadFormat::Json => payload.to_wire().to_string(),
        PayloadFormat::Trongo => match crate::trongo::to_trongo(payload) {
            Ok(t) => t,
            Err(e) => return shell_hook_error(spec, format!("could not write payload: {e}")),
        },
    };
    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c")
        .arg(&spec.command)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env("COSMOS_EVENT", spec.event.wire_name())
        .env("COSMOS_SESSION_ID", &payload.session_id)
        .env("COSMOS_CWD", cwd)
        .env("CLAUDE_PROJECT_DIR", cwd)
        .env("COSMOS_FILE_PATHS", payload.file_paths().join(" "));
    if let Some(t) = &payload.tool {
        cmd.env("COSMOS_TOOL_NAME", t);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return shell_hook_error(spec, format!("did not start: {e}")),
    };
    if let Some(mut stdin) = child.stdin.take() {
        tokio::spawn(async move {
            // The hook may exit without reading; that is its right.
            let _ = stdin.write_all(stdin_text.as_bytes()).await;
            let _ = stdin.shutdown().await;
        });
    }
    let timeout = Duration::from_millis(spec.timeout_ms.max(1));
    let output = tokio::select! {
        r = tokio::time::timeout(timeout, child.wait_with_output()) => match r {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => return shell_hook_error(spec, format!("failed: {e}")),
            Err(_) => return shell_hook_error(spec, format!("timed out after {timeout:?}")),
        },
        () = cancel.cancelled() => return shell_hook_error(spec, "interrupted"),
    };
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    match output.status.code() {
        Some(0) => {
            let mut response = HookResponse::from_stdout(&stdout).unwrap_or_else(|| {
                // Plain stdout is context for events that take input.
                let mut r = HookResponse::default();
                if matches!(
                    spec.event,
                    HookEvent::UserPromptSubmit | HookEvent::SessionStart
                ) && !stdout.trim().is_empty()
                {
                    r.additional_context = Some(stdout.trim().to_owned());
                }
                r
            });
            let block = (response.decision == Some(HookDecision::Block)).then(|| {
                response
                    .reason
                    .take()
                    .unwrap_or_else(|| "a hook blocked this".to_owned())
            });
            HookRun {
                response,
                block,
                warning: None,
            }
        }
        Some(2) => HookRun {
            response: HookResponse::default(),
            block: Some(if stderr.is_empty() {
                "a hook blocked this".to_owned()
            } else {
                stderr
            }),
            warning: None,
        },
        other => shell_hook_error(
            spec,
            format!(
                "exited with {}{}",
                other.map_or_else(|| "a signal".to_owned(), |c| c.to_string()),
                if stderr.is_empty() {
                    String::new()
                } else {
                    format!(": {stderr}")
                }
            ),
        ),
    }
}

/// Which project hook definitions the user has approved, by content hash.
///
/// A JSON file mapping a project's config directory to the hash of the hook
/// definitions that were approved. Change a hook and the hash changes, so the
/// approval no longer applies.
#[derive(Debug, Clone)]
pub struct HookTrust {
    path: PathBuf,
}

impl HookTrust {
    /// A store in the file `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `trusted-hooks.json` in the user config directory `opts` resolves.
    pub fn for_options(opts: &LoadOptions) -> Option<Self> {
        opts.resolved_user_dir()
            .map(|d| Self::new(d.join("trusted-hooks.json")))
    }

    fn read(&self) -> BTreeMap<String, String> {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Whether `hash` was approved for the project at `project`.
    pub fn is_trusted(&self, project: &Path, hash: &str) -> bool {
        self.read()
            .get(&project.to_string_lossy().into_owned())
            .is_some_and(|h| h == hash)
    }

    /// Approves `hash` for the project at `project`.
    pub fn trust(&self, project: &Path, hash: &str) -> Result<()> {
        let mut map = self.read();
        map.insert(project.to_string_lossy().into_owned(), hash.to_owned());
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e))?;
        }
        let text = serde_json::to_string_pretty(&map)?;
        std::fs::write(&self.path, format!("{text}\n")).map_err(|e| CoreError::io(&self.path, e))
    }
}

/// Project hooks that were found but not trusted.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PendingHooks {
    /// The project's config directory.
    pub project: PathBuf,
    /// The hash to pass to [`HookTrust::trust`] once the user approves.
    pub hash: String,
    /// What would run.
    pub hooks: Vec<HookSpec>,
}

/// The result of [`LoadedHooks::load`].
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct LoadedHooks {
    /// Hooks that may run: the user's, and the project's when trusted.
    pub active: Vec<HookSpec>,
    /// Untrusted project hooks, for the frontend to show.
    pub pending: Option<PendingHooks>,
    /// Files that did not load, and the note about untrusted hooks.
    pub warnings: Vec<String>,
}

fn hooks_in_dir(ev: &ConfigEvaluator, dir: &Path, warnings: &mut Vec<String>) -> Vec<HookSpec> {
    let mut found: Vec<Value> = Vec::new();
    let mut note = |what: String| warnings.push(what);
    for name in ["config.trongo", "config.toml", "config.json"] {
        let path = dir.join(name);
        if !path.is_file() {
            continue;
        }
        let value = match name {
            "config.trongo" => ev.eval_file(dir, name),
            _ => std::fs::read_to_string(&path)
                .map_err(|e| CoreError::io(&path, e))
                .and_then(|t| {
                    if name.ends_with("toml") {
                        toml::from_str::<Value>(&t).map_err(|e| CoreError::Config(e.to_string()))
                    } else {
                        serde_json::from_str(&t).map_err(Into::into)
                    }
                }),
        };
        match value {
            Ok(v) => {
                if let Some(h) = v.get("hooks").cloned() {
                    found.push(h);
                }
            }
            Err(e) => note(format!("{}: {e}", path.display())),
        }
        break;
    }
    if dir.join("hooks.trongo").is_file() {
        match ev.eval_file(dir, "hooks.trongo") {
            Ok(v) => found.push(v),
            Err(e) => note(format!("{}: {e}", dir.join("hooks.trongo").display())),
        }
    }
    let mut out = Vec::new();
    for v in found {
        match HookSpec::from_config(v) {
            Ok(mut specs) => out.append(&mut specs),
            Err(e) => note(format!("{}: {e}", dir.display())),
        }
    }
    out
}

fn hash_hooks(hooks: &[HookSpec]) -> String {
    let text = serde_json::to_string(hooks).unwrap_or_default();
    let digest = Sha256::digest(text.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

impl LoadedHooks {
    /// Reads `hooks` from `config.{trongo,toml,json}` and `hooks.trongo` in the
    /// user and project config directories, and gates the project's on
    /// `opts.trust_project` or `trust`.
    pub fn load(opts: &LoadOptions, trust: Option<&HookTrust>) -> Self {
        let snapshot = opts
            .snapshot
            .clone()
            .unwrap_or_else(|| EvalSnapshot::capture(&opts.cwd));
        let ev = ConfigEvaluator::new(snapshot);
        let mut out = Self::default();
        if let Some(u) = opts.resolved_user_dir().filter(|d| d.is_dir()) {
            out.active = hooks_in_dir(&ev, &u, &mut out.warnings);
        }
        let project = opts.resolved_project_dir();
        if project.is_dir() {
            let hooks = hooks_in_dir(&ev, &project, &mut out.warnings);
            if !hooks.is_empty() {
                let hash = hash_hooks(&hooks);
                let trusted =
                    opts.trust_project || trust.is_some_and(|t| t.is_trusted(&project, &hash));
                if trusted {
                    out.active.extend(hooks);
                } else {
                    out.warnings.push(format!(
                        "{} project hook(s) in {} were not run: they are not trusted yet (hash {})",
                        hooks.len(),
                        project.display(),
                        &hash[..12]
                    ));
                    out.pending = Some(PendingHooks {
                        project,
                        hash,
                        hooks,
                    });
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matcher_alternatives_and_wildcards() {
        let h = HookSpec::new(HookEvent::PostToolUse, "true").matcher("edit|multi_edit|write");
        assert!(h.matches_tool("edit"));
        assert!(h.matches_tool("Write"));
        assert!(!h.matches_tool("bash"));
        let w = HookSpec::new(HookEvent::PreToolUse, "true").matcher("mcp_*");
        assert!(w.matches_tool("mcp_github"));
        assert!(HookSpec::new(HookEvent::PreToolUse, "true").matches_tool("anything"));
    }

    #[test]
    fn config_shapes() {
        let a = HookSpec::from_config(json!({
            "post-tool-use": [{"matcher": "edit", "command": "fmt", "timeout-ms": 10000}]
        }))
        .unwrap();
        assert_eq!(a[0].event, HookEvent::PostToolUse);
        assert_eq!(a[0].timeout_ms, 10000);
        let b = HookSpec::from_config(json!({
            "PostToolUse": [{"matcher": "Edit", "hooks": [{"type": "command", "command": "fmt", "timeout": 5}]}]
        }))
        .unwrap();
        assert_eq!(b[0].matcher.as_deref(), Some("Edit"));
        assert_eq!(b[0].timeout_ms, 5000);
        assert!(HookSpec::from_config(json!({"bogus": []})).is_err());
        assert!(HookSpec::from_config(json!({"stop": [{"command": ""}]})).is_err());
    }

    #[test]
    fn responses_parse_claude_code_shapes() {
        let r = HookResponse::from_stdout(
            r#"{"decision":"block","reason":"no","hookSpecificOutput":{"additionalContext":"ctx","updatedInput":{"a":1}}}"#,
        )
        .unwrap();
        assert_eq!(r.decision, Some(HookDecision::Block));
        assert_eq!(r.additional_context.as_deref(), Some("ctx"));
        assert_eq!(r.updated_input, Some(json!({"a": 1})));
        let r = HookResponse::from_stdout(r#"{"decision":"approve"}"#).unwrap();
        assert_eq!(r.decision, Some(HookDecision::Allow));
        assert!(HookResponse::from_stdout("plain text").is_none());
    }
}
