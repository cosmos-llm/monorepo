//! # cosmos-llm-agent-core
//!
//! The headless heart of a coding agent, between the tools and a frontend:
//!
//! - [`protocol`]: [`Op`]s in, [`Event`]s out (210). A [`Core`] owns a
//!   [`Thread`](cosmos_llm_tool::Thread), runs the loop, and speaks only this
//!   protocol.
//! - [`session`] and [`record`]: an append-only JSONL log with resume,
//!   continue and fork (200).
//! - [`policy`]: approval modes, rules, shell classification and path scoping
//!   (150), plugged in as an [`Approver`](cosmos_llm_tool::Approver) in front
//!   of the interactive one ([`approval`]).
//! - [`budget`]: how full the window is, before sending (160).
//! - [`config`] and [`eval`]: [`CoreConfig`] loaded from `.trongo` (or TOML),
//!   evaluated in a sandbox with a few pure host builtins (005).
//!
//! ```no_run
//! # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! use std::sync::Arc;
//! use cosmos_llm::Client;
//! use cosmos_llm_agent_core::{Core, CoreConfig, Event, LoadOptions};
//! use cosmos_llm_tool::Registry;
//!
//! let cwd = std::env::current_dir()?;
//! let loaded = CoreConfig::load(&LoadOptions::new(&cwd))?;
//! let client: Arc<dyn cosmos_llm_tool::Completer> = Arc::new(Client::new("anthropic", "key")?);
//! let (handle, mut events) = Core::builder(loaded.config)
//!     .completer(client)
//!     .tools(Registry::new())
//!     .warnings(loaded.warnings)
//!     .start()?;
//! handle.user_text("hello").await?;
//! while let Some(ev) = events.recv().await {
//!     if matches!(ev, Event::RunFinished { .. }) { break; }
//! }
//! # Ok(()) }
//! ```
//!
//! IR types are plain serde. See `tests/golden/` for JSON and TrongoScript
//! fixtures of each, and [`trongo`] for loading and dumping them.

pub mod approval;
pub mod budget;
pub mod command;
#[cfg(feature = "cli")]
pub mod cli;
mod defs;
pub mod compaction;
pub mod config;
pub mod core;
pub mod error;
pub mod eval;
pub mod hooks;
pub mod exec;
pub mod policy;
pub mod protocol;
pub mod record;
pub mod session;
pub mod subagent;
pub mod trongo;

pub use approval::InteractiveApprover;
pub use command::{CommandSet, CommandSourceReg, CommandSpec, Expanded, Expansion};
pub use hooks::{
    HookDecision, HookEvent, HookOutcome, HookPayload, HookResponse, HookRunner, HookSpec,
    HookTrust, LoadedHooks, PayloadFormat, PendingHooks,
};
pub use subagent::{AgentSet, AgentSpec, SubagentOptions, SUBAGENT_ID_SEP};
pub use budget::{ContextBudget, ContextTracker, Pressure};
pub use compaction::{
    CompactionConfig, CompactionOutcome, CompactionPlan, CompactionReason, Compactor,
    CompactorContext, CompactorPlugin, DropCompactor, SummarizingCompactor,
};
pub use config::{
    append_permission_rule, ConfigFormat, ConfigSource, ContextConfig, CoreConfig, Layer,
    LoadOptions, LoadedConfig, SessionConfig,
};
pub use core::{Core, CoreBuilder, CoreHandle, ResumeFrom};
pub use error::{CoreError, Result};
pub use eval::{ConfigEvaluator, EvalSnapshot};
pub use exec::{run_exec, ExecOptions, ExecResult, ExecStop, OutputFormat};
pub use policy::{
    ApprovalMode, PolicyApprover, PolicyEngine, PolicySpec, PolicyVerdict, Rule, ShellParse,
    ShellParser,
};
pub use protocol::{Envelope, Event, Op, PROTOCOL_VERSION};
pub use record::{BlobRef, DisplayItem, FileChange, Fork, MsgId, Record, RECORD_VERSION};
pub use session::{LoadedSession, SessionLog, SessionStore, SessionSummary, SessionWriter};
