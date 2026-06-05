//! # cosmos-llm-virtual-filesystem
//!
//! A minimal in-memory virtual filesystem for LLM contexts.
//!
//! Mirrors the Ruby `cosmos-llm-virtual-filesystem` gem. Provides nested
//! directory/file structures with path-based navigation, metadata, and content
//! management. All objects are immutable once built.
//!
//! ## Quick start
//!
//! ```rust
//! use cosmos_llm_virtual_filesystem::Filesystem;
//!
//! let fs = Filesystem::build("/", |fs| {
//!     fs.directory("src", |src| {
//!         src.file("main.rs", Some("fn main() {}"), Default::default());
//!     });
//! });
//!
//! let file = fs.find_file("src/main.rs").unwrap();
//! assert_eq!(file.content.as_deref(), Some("fn main() {}"));
//! ```

pub mod error;
pub mod filesystem;

pub use error::VfsError;
pub use filesystem::{Filesystem, VirtualFile};
