//! # cosmos-llm
//!
//! A unified Rust client for multiple LLM providers.
//!
//! `cosmos-llm` mirrors the design of the Ruby and JavaScript siblings in the
//! Cosmos-LLM monorepo: one API surface, pluggable provider backends.
//!
//! ## Quick start
//!
//! ```no_run
//! use cosmos_llm::{Client, Message, CompletionRequest};
//!
//! # tokio_test::block_on(async {
//! // Reads OPENAI_API_KEY from the environment.
//! let client = Client::new("openai", std::env::var("OPENAI_API_KEY").unwrap())
//!     .unwrap()
//!     .with_model("gpt-4o");
//!
//! let text = client.complete("What is 2 + 2?").await.unwrap();
//! println!("{text}");
//! # })
//! ```
//!
//! ## Environment variables
//!
//! | Variable | Provider |
//! |---|---|
//! | `OPENAI_API_KEY` or `CLLM__OPENAI__API_KEY` | OpenAI |
//! | `ANTHROPIC_API_KEY` or `CLLM__ANTHROPIC__API_KEY` | Anthropic |
//! | `OPENROUTER_API_KEY` or `CLLM__OPENROUTER__API_KEY` | OpenRouter |
//!
//! ## Supported providers
//!
//! | Name | Completion | Tools | Streaming | Models |
//! |---|---|---|---|---|
//! | `openai` | ✓ | ✓ | — | ✓ |
//! | `anthropic` | ✓ | ✓ | — | ✓ (static list) |
//! | `openrouter` | ✓ | ✓ | — | ✓ |
//!
//! Streaming is not implemented for any provider yet;
//! [`Client::can_stream`] reports `false` everywhere.
//!
//! ## Reusing opencode credentials
//!
//! If you already ran `opencode auth login`, those keys can be loaded instead
//! of re-exporting them. This is opt-in — [`Config::new`] never reads the file.
//!
//! ```no_run
//! use cosmos_llm::{Client, Config};
//!
//! let mut config = Config::new();
//! config.load_opencode_auth().unwrap();
//! let client = Client::from_config(config, "openrouter").unwrap();
//! ```
//!
//! See the [`opencode`] module for details.

pub mod client;
pub mod config;
pub mod error;
pub mod opencode;
pub mod providers;
pub mod types;

pub use client::Client;
pub use config::Config;
pub use error::CosmosError;
pub use types::{
    Choice, CompletionRequest, CompletionResponse, Message, StreamChunk, ToolCall, Usage,
};
