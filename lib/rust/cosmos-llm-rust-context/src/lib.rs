//! # cosmos-llm-context
//!
//! A builder-pattern DSL for constructing structured LLM prompt contexts.
//!
//! Mirrors the Ruby `cosmos-llm-ruby-context` gem. Assemble ordered
//! [`Block`]s and an optional virtual filesystem, then render them for
//! different providers.
//!
//! ## Quick start
//!
//! ```rust
//! use cosmos_llm_context::{ContextBuilder, RenderFormat};
//!
//! let ctx = ContextBuilder::new()
//!     .block("system", "You are a helpful assistant", Default::default())
//!     .block("user", "What is Rust?", Default::default())
//!     .render(RenderFormat::Default);
//!
//! assert!(ctx.contains("You are a helpful assistant"));
//! ```

pub mod block;
pub mod builder;
pub mod environment;
pub mod error;
pub mod instructions;
pub mod plan;
pub mod renderers;
#[cfg(feature = "trongo")]
pub mod trongo;

pub use block::Block;
pub use builder::ContextBuilder;
pub use environment::{probe_environment, EnvironmentFacts, GitFacts, ProbeOptions};
pub use error::ContextError;
pub use instructions::{
    find_git_root, load_dir_instructions, load_instructions, FileOrigin, InstructionOptions,
    Instructions, LoadedFile,
};
pub use plan::{CacheHint, PromptBlock, PromptPlan, PromptSection, SectionKind, Stability};
pub use renderers::RenderFormat;
