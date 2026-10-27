//! Headless `exec` driver (220): one prompt in, a result and an exit code out.
//!
//! [`run_exec`] drives a [`Core`](crate::Core) the way a terminal UI would,
//! minus the terminal. It sends one input, relays [`Event`]s in the chosen
//! [`OutputFormat`], answers every approval question with a denial (nobody is
//! there to ask), turns a cancelled token into `Op::Interrupt`, and returns an
//! [`ExecResult`]. The command line in [`cli`](crate::cli) is a thin shell
//! around it.
//!
//! Output formats:
//!
//! | Format | stdout | stderr |
//! |---|---|---|
//! | `text` | the assistant's text, streamed | one line per tool call, warnings, errors |
//! | `json` | the final [`ExecResult`] as one JSON object | nothing |
//! | `jsonl` | every [`Event`] as a line, then `{"event":"exec_result",...}` | nothing |
//! | `trongo` | every event, then the result, as TrongoScript data literals | nothing |
//!
//! `trongo` follows 005 (per-event data literals), which supersedes the
//! "final object only" wording in 220; `json` is the final-object form.
//!
//! Exit codes ([`ExecResult::exit_code`]): `0` finished, `1` error, `2`
//! stopped by the turn or cost limit, `130` interrupted.

use std::collections::HashMap;
use std::io::Write;
use std::str::FromStr;
use std::time::Duration;

use cosmos_llm_tool::{AgentEvent, Decision, StopReason, TokenUsage};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::mpsc::Receiver;
use tokio_util::sync::CancellationToken;

use crate::core::CoreHandle;
use crate::error::{CoreError, Result};
use crate::protocol::{Event, Op};
use crate::trongo::to_trongo;

/// The [`ExecResult`] schema version.
pub const EXEC_VERSION: u32 = 1;

/// What goes to stdout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum OutputFormat {
    /// The assistant's text; tool activity on stderr.
    #[default]
    Text,
    /// One JSON object at the end.
    Json,
    /// One JSON event per line.
    Jsonl,
    /// Events as TrongoScript data literals.
    Trongo,
}

impl FromStr for OutputFormat {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, String> {
        match s {
            "text" => Ok(Self::Text),
            "json" => Ok(Self::Json),
            "jsonl" => Ok(Self::Jsonl),
            "trongo" => Ok(Self::Trongo),
            other => Err(format!(
                "unknown output format '{other}' (text, json, jsonl, trongo)"
            )),
        }
    }
}

/// Why the run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ExecStop {
    /// The model finished.
    Finished,
    /// The turn limit (`max_steps`) was reached with the model still working.
    MaxTurns,
    /// The cost limit was reached.
    MaxCost,
    /// Interrupted (SIGINT).
    Interrupted,
    /// A failure; see [`ExecResult::error`].
    Error,
}

/// The final object of a headless run (220): the IR behind `--output json`,
/// `jsonl`'s last line and `trongo`'s last literal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ExecResult {
    /// [`EXEC_VERSION`].
    pub v: u32,
    /// The assistant's last text.
    pub result: String,
    /// The session id (also the log file's id).
    pub session_id: String,
    /// Model turns taken.
    pub turns: u32,
    /// Tokens used.
    pub usage: TokenUsage,
    /// Dollars charged, when the provider reports a price.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    /// Why it stopped.
    pub stop_reason: ExecStop,
    /// The failure, when `stop_reason` is `error`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Tool calls that needed approval and were denied.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub denied: Vec<String>,
}

impl ExecResult {
    /// A result with no output yet.
    pub fn new(stop_reason: ExecStop) -> Self {
        Self {
            v: EXEC_VERSION,
            result: String::new(),
            session_id: String::new(),
            turns: 0,
            usage: TokenUsage::default(),
            cost: None,
            stop_reason,
            error: None,
            denied: Vec::new(),
        }
    }

    /// `0` finished, `1` error, `2` turn or cost limit, `130` interrupted.
    pub fn exit_code(&self) -> u8 {
        match self.stop_reason {
            ExecStop::Finished => 0,
            ExecStop::MaxTurns | ExecStop::MaxCost => 2,
            ExecStop::Interrupted => 130,
            _ => 1,
        }
    }

    fn as_event_value(&self) -> Value {
        let mut v = serde_json::to_value(self).unwrap_or(Value::Null);
        if let Value::Object(m) = &mut v {
            m.insert("event".into(), json!("exec_result"));
        }
        v
    }
}

/// Settings for one headless run. Turn and mode limits are [`CoreConfig`](crate::CoreConfig)
/// (`max_steps`, `permissions.mode`); only what the driver itself enforces is here.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ExecOptions {
    /// What to print.
    pub output: OutputFormat,
    /// Stop (interrupt) once the run has cost this many dollars. Needs a
    /// provider that reports prices.
    pub max_cost: Option<f64>,
    /// How long to wait for the core to flush and shut down.
    pub shutdown_timeout: Duration,
}

impl Default for ExecOptions {
    fn default() -> Self {
        Self {
            output: OutputFormat::Text,
            max_cost: None,
            shutdown_timeout: Duration::from_secs(10),
        }
    }
}

impl ExecOptions {
    /// Options for `output` with no cost limit.
    pub fn new(output: OutputFormat) -> Self {
        Self {
            output,
            ..Self::default()
        }
    }

    /// Sets the cost limit.
    #[must_use]
    pub fn max_cost(mut self, dollars: f64) -> Self {
        self.max_cost = Some(dollars);
        self
    }
}

/// The reason a denied call gives the model.
pub const NON_INTERACTIVE_DENIAL: &str = "not permitted in non-interactive mode: this call needs approval and there is no one to ask. Use a different approach, or ask the user to rerun with a more permissive --mode or an allow rule.";

fn io_err(e: std::io::Error) -> CoreError {
    CoreError::io("output", e)
}

fn short(s: &str, max: usize) -> String {
    let one = s.lines().next().unwrap_or("");
    if one.chars().count() > max {
        let cut: String = one.chars().take(max).collect();
        format!("{cut}...")
    } else {
        one.to_owned()
    }
}

struct Sink<'a, W: Write, E: Write> {
    format: OutputFormat,
    out: &'a mut W,
    err: &'a mut E,
}

impl<W: Write, E: Write> Sink<'_, W, E> {
    fn event(&mut self, ev: &Event, tools: &mut HashMap<String, String>) -> Result<()> {
        match self.format {
            OutputFormat::Jsonl => {
                writeln!(self.out, "{}", serde_json::to_string(ev)?).map_err(io_err)?;
            }
            OutputFormat::Trongo => {
                writeln!(self.out, "{}", to_trongo(ev)?.trim_end()).map_err(io_err)?;
            }
            OutputFormat::Json => {}
            OutputFormat::Text => self.text(ev, tools)?,
        }
        Ok(())
    }

    fn text(&mut self, ev: &Event, tools: &mut HashMap<String, String>) -> Result<()> {
        match ev {
            Event::Agent(AgentEvent::TextDelta { text }) => {
                write!(self.out, "{text}").map_err(io_err)?;
            }
            Event::Agent(AgentEvent::TurnFinished { .. }) => {}
            Event::Agent(AgentEvent::ToolCallReady {
                call_id,
                name,
                input,
            }) => {
                tools.insert(call_id.clone(), name.clone());
                writeln!(self.err, "[tool] {name} {}", short(&input.to_string(), 100))
                    .map_err(io_err)?;
            }
            Event::Agent(AgentEvent::ToolFinished { call_id, result }) => {
                let name = tools.get(call_id).map_or("tool", String::as_str);
                let status = if result.is_error { "error" } else { "ok" };
                writeln!(
                    self.err,
                    "[tool] {name} {status}: {}",
                    short(&result.as_text(), 100)
                )
                .map_err(io_err)?;
            }
            Event::Warning { message } => {
                writeln!(self.err, "warning: {message}").map_err(io_err)?;
            }
            Event::Error { message, .. } => {
                writeln!(self.err, "error: {message}").map_err(io_err)?;
            }
            Event::CompactionStarted { .. } => {
                writeln!(self.err, "[context] compacting").map_err(io_err)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn finish(&mut self, r: &ExecResult) -> Result<()> {
        match self.format {
            OutputFormat::Text => writeln!(self.out).map_err(io_err)?,
            OutputFormat::Json => {
                writeln!(self.out, "{}", serde_json::to_string(r)?).map_err(io_err)?;
            }
            OutputFormat::Jsonl => {
                writeln!(self.out, "{}", r.as_event_value()).map_err(io_err)?;
            }
            OutputFormat::Trongo => {
                writeln!(self.out, "{}", to_trongo(&r.as_event_value())?.trim_end())
                    .map_err(io_err)?;
            }
        }
        self.out.flush().map_err(io_err)
    }
}

/// Runs one prompt on a started core and returns the result.
///
/// `events` is the receiver [`CoreBuilder::start`](crate::CoreBuilder::start)
/// returned with `handle`. Cancelling `interrupt` sends `Op::Interrupt`; the
/// run then ends as [`ExecStop::Interrupted`] once the core has made its
/// history valid and flushed the session. Approvals are always denied with
/// [`NON_INTERACTIVE_DENIAL`], and the run goes on.
///
/// # Errors
///
/// Fails only when writing the output fails or the core is already gone;
/// failures of the run itself are reported as [`ExecStop::Error`].
pub async fn run_exec<W: Write, E: Write>(
    handle: &CoreHandle,
    events: &mut Receiver<Event>,
    prompt: &str,
    opts: &ExecOptions,
    interrupt: &CancellationToken,
    out: &mut W,
    err: &mut E,
) -> Result<ExecResult> {
    let mut sink = Sink {
        format: opts.output,
        out,
        err,
    };
    let mut tools: HashMap<String, String> = HashMap::new();
    let mut result = ExecResult::new(ExecStop::Error);
    let mut turn_text = String::new();
    let mut spent: Option<f64> = None;
    let mut interrupted = false;
    let mut cost_hit = false;
    let mut sent = false;
    let mut finished = false;

    while !finished {
        let ev = tokio::select! {
            ev = events.recv() => ev,
            () = interrupt.cancelled(), if !interrupted => {
                interrupted = true;
                if sent { handle.interrupt().await?; } else { finished = true; result.stop_reason = ExecStop::Interrupted; }
                continue;
            }
        };
        let Some(ev) = ev else {
            result.error = Some("the core stopped unexpectedly".into());
            break;
        };
        sink.event(&ev, &mut tools)?;
        match ev {
            Event::SessionConfigured { session_id, .. } => {
                result.session_id = session_id;
                if !sent && !interrupted {
                    handle.user_text(prompt).await?;
                    sent = true;
                }
            }
            Event::ApprovalRequested {
                call_id,
                tool,
                input,
                ..
            } => {
                result
                    .denied
                    .push(format!("{tool} {}", short(&input.to_string(), 80)));
                handle
                    .approve(call_id, Decision::deny(NON_INTERACTIVE_DENIAL))
                    .await?;
            }
            Event::Agent(AgentEvent::TurnStarted { .. }) => turn_text.clear(),
            Event::Agent(AgentEvent::TextDelta { text }) => turn_text.push_str(&text),
            Event::Agent(AgentEvent::Usage { total, .. }) => {
                spent = total.cost;
                if let (Some(limit), Some(cost)) = (opts.max_cost, total.cost) {
                    if cost >= limit && !cost_hit {
                        cost_hit = true;
                        handle.interrupt().await?;
                    }
                }
            }
            Event::Error { message, fatal, .. } => {
                result.error = Some(message);
                if fatal {
                    break;
                }
            }
            Event::RunFinished {
                reason,
                steps,
                usage,
            } => {
                result.turns = steps;
                result.usage = usage;
                result.stop_reason = match reason {
                    Some(StopReason::Finished) => ExecStop::Finished,
                    Some(StopReason::Steps) => ExecStop::MaxTurns,
                    Some(StopReason::Budget) => ExecStop::MaxCost,
                    Some(StopReason::Interrupted) if cost_hit => ExecStop::MaxCost,
                    Some(StopReason::Interrupted) => ExecStop::Interrupted,
                    Some(StopReason::Aborted) => {
                        result
                            .error
                            .get_or_insert_with(|| "the run was aborted".into());
                        ExecStop::Error
                    }
                    _ => ExecStop::Error,
                };
                if result.stop_reason == ExecStop::Error && result.error.is_none() {
                    result.error = Some("the run failed".into());
                }
                finished = true;
            }
            _ => {}
        }
    }

    result.result = turn_text;
    result.cost = result.usage.cost.or(spent);

    // Flush the session: ask the core to stop and wait for it to say so.
    if handle.send(Op::Shutdown).await.is_ok() {
        let drain = async {
            while let Some(ev) = events.recv().await {
                if matches!(ev, Event::ShutdownComplete) {
                    break;
                }
            }
        };
        let _ = tokio::time::timeout(opts.shutdown_timeout, drain).await;
    }
    sink.finish(&result)?;
    Ok(result)
}
