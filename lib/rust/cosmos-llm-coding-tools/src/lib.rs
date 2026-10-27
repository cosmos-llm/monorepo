//! # cosmos-llm-coding-tools
//!
//! Tools a coding agent needs, on top of `cosmos-llm-tool`:
//!
//! - [`DiskFs`], a [`ToolFs`](cosmos_llm_tool::fs::ToolFs) over a real working
//!   tree, with strict path confinement and atomic writes ([`disk`]).
//! - [`FileTracker`] and the [`FileDiff`] / [`TurnChanges`] IR types: staleness
//!   checks, turn diffs and undo ([`tracker`], [`diff`]).
//! - The `read`, `write`, `edit` and `multi_edit` tools, with a
//!   read-before-write guard ([`tools`], [`edit`]).
//! - `glob`, `grep` and fuzzy path search ([`search`]).
//! - `bash`, `job_output` and `job_kill`, plus the `ShellCommandIr` parse tree
//!   the safety policy classifies ([`shell`]).
//!
//! ```rust
//! use std::sync::Arc;
//! use cosmos_llm_coding_tools::{CodingTools, DiskFs};
//! use cosmos_llm_tool::Registry;
//!
//! let dir = tempfile::tempdir().unwrap();
//! let tools = CodingTools::new(Arc::new(DiskFs::new(dir.path()).unwrap()));
//! let mut registry = Registry::new();
//! tools.register_all(&mut registry);
//! assert_eq!(registry.count(), 9);
//! ```
//!
//! The IR types are plain serde. With the default `trongo` feature they also
//! load from and dump to TrongoScript data ([`trongo`]).

pub mod diff;
pub mod disk;
pub mod edit;
pub mod search;
pub mod shell;
pub mod tools;
pub mod tracker;

#[cfg(feature = "trongo")]
pub mod trongo;

pub use diff::{ChangeKind, FileDiff, Hunk};
pub use disk::DiskFs;
pub use edit::EditSpec;
pub use search::{FileIndex, GrepBackend};
pub use shell::{parse_shell, ShellCommandIr, ShellConfig, ShellRun, ShellTools};
pub use tools::{CodingTools, EditPlan};
pub use tracker::{FileTracker, RevertReport, SharedTracker, TurnChanges, TurnFile};
