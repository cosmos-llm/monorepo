//! Ready-to-use preset tools that operate on a virtual filesystem.
//!
//! All tools take a shared reference to a [`Filesystem`] and return a
//! [`ToolDefinition`] whose handler is already wired up. The filesystem
//! provides the sandbox; agents can only touch what was explicitly granted.
//!
//! [`Filesystem`]: cosmos_llm_virtual_filesystem::Filesystem

mod glob;
mod grep;
mod list;
mod read;
mod write;

#[cfg(feature = "webfetch")]
mod webfetch;

pub use glob::glob_tool;
pub use grep::grep_tool;
pub use list::list_tool;
pub use read::read_tool;
pub use write::write_tool;

#[cfg(feature = "webfetch")]
pub use webfetch::webfetch_tool;
