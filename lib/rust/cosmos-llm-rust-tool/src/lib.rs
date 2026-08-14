//! # cosmos-llm-tool
//!
//! Function-calling layer and preset tools for LLM agents.
//!
//! Mirrors the Ruby `cosmos-llm-ruby-tool` and `cosmos-llm-ruby-tool-preset`
//! gems. Define tools with typed parameters, register them in a [`Registry`],
//! and execute them via the [`Executor`].
//!
//! ## Quick start
//!
//! ```rust
//! use cosmos_llm_tool::{ToolDefinition, ParameterType, Registry, Executor};
//! use serde_json::json;
//!
//! let tool = ToolDefinition::new("greet")
//!     .description("Greets a user by name")
//!     .param("name", ParameterType::String, true, "The user's name")
//!     .handler(|params| {
//!         let name = params["name"].as_str().unwrap_or("world");
//!         Ok(json!(format!("Hello, {name}!")))
//!     });
//!
//! let result = Executor::execute(&tool, &json!({ "name": "Alice" })).unwrap();
//! assert_eq!(result, json!("Hello, Alice!"));
//! ```
//!
//! ## Async tools
//!
//! Most real tools are I/O-bound. Give those an
//! [`async_handler`](ToolDefinition::async_handler), an optional
//! [`timeout`](ToolDefinition::timeout), and run them with
//! [`Executor::execute_async`] — or a whole batch concurrently with
//! [`Executor::execute_all`], which is what an agent loop wants when a model
//! requests several tools at once.
//!
//! ```rust
//! # #[cfg(feature = "async")] {
//! use std::time::Duration;
//! use cosmos_llm_tool::{Executor, ParameterType, ToolDefinition};
//! use serde_json::json;
//!
//! # tokio_test::block_on(async {
//! let fetch = ToolDefinition::new("fetch")
//!     .param("id", ParameterType::Integer, true, "Record to fetch")
//!     .timeout(Duration::from_secs(5))
//!     .async_handler(|params| Box::pin(async move {
//!         // Real work would await a network call here.
//!         Ok(json!({ "id": params["id"], "found": true }))
//!     }));
//!
//! let results = Executor::execute_all(
//!     vec![(&fetch, json!({"id": 1})), (&fetch, json!({"id": 2}))],
//!     4,
//! )
//! .await;
//!
//! assert_eq!(results.len(), 2);
//! assert_eq!(results[0].as_ref().unwrap()["id"], 1);
//! # })
//! # }
//! ```
//!
//! Synchronous tools stay synchronous — parsing and arithmetic gain nothing
//! from a future — and both kinds can share one registry and one batch.
//!
//! The `async` feature is on by default; turning it off leaves the sync half
//! intact with no `futures` or runtime dependency.
//!
//! ## Running an agent
//!
//! Defining tools and rendering their schemas needs no LLM client — a consumer
//! who hands those schemas to some other client stops here. Enable the `agent`
//! feature to also get [`AgentLoop`], which drives a model through the toolset
//! until it stops calling tools:
//!
//! ```toml
//! cosmos-llm-tool = { version = "0.1", features = ["agent"] }
//! ```
//!
//! ```rust
//! # #[cfg(feature = "agent")]
//! # {
//! use cosmos_llm_tool::{AgentLoop, Registry, Session, StopReason};
//! use std::sync::Arc;
//!
//! # let registry = Registry::new();
//! // Caps the tools enforce, and a budget the loop enforces.
//! let session = Arc::new(Session::with_budgets([("search", 40), ("open", 25)]));
//!
//! let agent = AgentLoop::new(registry)
//!     .with_model("claude-opus-4")
//!     .with_system("You are a research assistant.")
//!     .with_session(Arc::clone(&session))
//!     .with_budget(|usage| usage.total_tokens < 100_000)
//!     .with_max_steps(20);
//!
//! // Against OpenRouter, which reports the charged price, budget in dollars
//! // instead. Other providers report no price, so `cost` stays `None` there
//! // and a dollar ceiling would never trip — budget on tokens for those.
//! # let registry2 = Registry::new();
//! let priced = AgentLoop::new(registry2)
//!     .with_budget(|usage| usage.cost_or_zero() < 0.50);
//!
//! # async fn run(agent: AgentLoop, client: &cosmos_llm::Client) {
//! let outcome = agent.run(client, "What changed in Q3?").await.unwrap();
//! match outcome.reason {
//!     StopReason::Finished => println!("{}", outcome.text),
//!     StopReason::Steps => eprintln!("hit the step cap"),
//!     StopReason::Budget => eprintln!("hit the token ceiling"),
//! }
//! # }
//! # }
//! ```
//!
//! The loop is provider-neutral: it speaks the normalized
//! `cosmos_llm::ToolCall`, so the same code runs against OpenAI and Anthropic
//! without a dialect switch. [`Session`] and [`Progress`] need no client and
//! are available without the feature.

pub mod definition;
pub mod error;
pub mod executor;
pub mod parameter;
pub mod preset;
pub mod progress;
pub mod registry;
pub mod schemas;
pub mod session;

#[cfg(feature = "agent")]
pub mod agent;

pub use definition::{Handler, ToolDefinition};
pub use error::ToolError;
pub use executor::Executor;
pub use parameter::ParameterType;
pub use progress::Progress;
pub use registry::Registry;
pub use session::Session;

#[cfg(feature = "agent")]
pub use agent::{AgentLoop, LoopEvent, RunOutcome, StopReason, TokenUsage};
