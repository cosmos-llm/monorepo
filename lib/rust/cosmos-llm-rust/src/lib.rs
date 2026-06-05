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
//!
//! ## Supported providers
//!
//! | Name | Completion | Streaming | Models |
//! |---|---|---|---|
//! | `openai` | ✓ | ✓ | ✓ |
//! | `anthropic` | ✓ | — | ✓ (static list) |

pub mod client;
pub mod config;
pub mod error;
pub mod providers;
pub mod types;

pub use client::Client;
pub use config::Config;
pub use error::CosmosError;
pub use types::{Choice, CompletionRequest, CompletionResponse, Message, StreamChunk, Usage};
