//! The shell tools: `bash`, `job_output` and `job_kill` (TODO 120).
//!
//! - [`ShellTools`] binds them to one working tree, one [`FileTracker`] and
//!   one table of background jobs.
//! - [`parse_shell`] lowers a command line into [`ShellCommandIr`], the tree
//!   the safety policy classifies.
//! - [`ShellRun`] is the display IR a `bash` call puts in
//!   `ToolOutcome::display`.
//!
//! Each `bash` call is a fresh `bash -c` in its own process group. The tool
//! tracks the working directory between calls (a `pwd` trailer, kept only if
//! the directory is still inside the allowed roots); environment changes are
//! not carried over. Stdout and stderr are merged in arrival order.
//!
//! Nothing here is sandboxed: the tool runs what the model asks, and the
//! approval policy is the only gate.

mod ir;
mod parse;
mod run;
mod tools;

pub use ir::{
    Assignment, ChainLink, ChainOp, Expansion, ProcessDirection, Redirect, RedirectOp,
    ShellCommandIr, ShellRun, ShellStatus, SimpleCommand, Word,
};
pub use parse::{parse_shell, ShellParseError};
pub use tools::{ShellConfig, ShellState, ShellTools};
