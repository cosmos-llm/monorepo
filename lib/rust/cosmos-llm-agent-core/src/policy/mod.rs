//! Approval policy (150): modes, rules, shell classification, path scoping.
//!
//! [`PolicySpec`] is the IR config lowers into. [`PolicyEngine`] evaluates it
//! against each [`ApprovalRequest`](cosmos_llm_tool::ApprovalRequest) and
//! [`PolicyApprover`] plugs the engine into the loop's approver slot, in front
//! of the interactive approver.
//!
//! `full_auto` is **not sandboxed**: the engine decides what to ask about,
//! nothing here confines a command that is allowed to run.

mod engine;
pub mod shell;
mod spec;

pub use engine::{PolicyApprover, PolicyEngine, PolicyVerdict};
pub use shell::{
    BasicShellParser, IrShellParser, Redirect, ShellParse, ShellParser, SimpleCommand,
};
pub use spec::{ApprovalMode, PolicySpec, Rule};
