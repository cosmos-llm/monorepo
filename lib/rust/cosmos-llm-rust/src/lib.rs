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
//! ## Streaming
//!
//! [`Client::stream`] returns a `Stream` of [`StreamChunk`]s as the model
//! generates them:
//!
//! ```no_run
//! use cosmos_llm::Client;
//! use futures_util::StreamExt;
//!
//! # tokio_test::block_on(async {
//! let client = Client::new("openai", std::env::var("OPENAI_API_KEY").unwrap())
//!     .unwrap()
//!     .with_model("gpt-4o");
//!
//! let mut stream = client.stream("Write a haiku about Rust").await.unwrap();
//! while let Some(chunk) = stream.next().await {
//!     print!("{}", chunk.unwrap().delta);
//! }
//! # })
//! ```
//!
//! Streamed tool calls arrive as [`ToolCallDelta`] fragments. Feed every chunk
//! to a [`StreamAccumulator`] to rebuild the complete
//! [`CompletionResponse`] — including assembled tool calls and token usage —
//! so the same tool-calling loop works with either mode.
//! [`Client::stream_to_completion`] does this for you while still handing each
//! chunk to a callback for live output.
//!
//! ## Environment variables
//!
//! | Variable | Provider |
//! |---|---|
//! | `OPENAI_API_KEY` or `CLLM__OPENAI__API_KEY` | OpenAI |
//! | `ANTHROPIC_API_KEY` or `CLLM__ANTHROPIC__API_KEY` | Anthropic |
//! | `OPENROUTER_API_KEY` or `CLLM__OPENROUTER__API_KEY` | OpenRouter |
//!
//! Each provider also accepts a `*_BASE_URL` variable
//! (`OPENAI_BASE_URL` / `CLLM__OPENAI__BASE_URL`, and so on) to override its
//! API root.
//!
//! ## Custom endpoints
//!
//! [`Client::new_with_base_url`] points a provider at a different API root —
//! an OpenAI-compatible server such as vLLM, llama.cpp, or Azure OpenAI, a
//! corporate proxy, or a mock server in tests. The provider name still selects
//! the wire format:
//!
//! ```no_run
//! use cosmos_llm::Client;
//!
//! let client = Client::new_with_base_url(
//!     "openai",
//!     "not-checked-locally",
//!     "http://localhost:8000/v1",
//! )
//! .unwrap()
//! .with_model("meta-llama/Llama-3-8b");
//! ```
//!
//! ## Supported providers
//!
//! | Name | Completion | Tools | Streaming | Models |
//! |---|---|---|---|---|
//! | `openai` | ✓ | ✓ | ✓ | ✓ |
//! | `anthropic` | ✓ | ✓ | ✓ | ✓ (static list) |
//! | `openrouter` | ✓ | ✓ | ✓ | ✓ |
//!
//! [`Client::can_stream`] reports whether the active provider implements
//! streaming.
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
pub mod routing;
pub mod sse;
pub mod types;

pub use client::Client;
pub use config::Config;
pub use error::{CosmosError, SYSTEM};
pub use providers::{resolve_with_base_url, CompletionStream};
pub use routing::{DataCollection, MaxPrice, OpenRouterRouting, ProviderSort, SortPartition};
pub use types::{
    Choice, CompletionRequest, CompletionResponse, Message, RouteInfo, StreamAccumulator,
    StreamChunk, ToolCall, ToolCallDelta, Usage,
};
