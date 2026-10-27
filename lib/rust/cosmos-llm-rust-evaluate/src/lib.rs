//! # cosmos-llm-evaluate
//!
//! Scores LLM programs against labeled devsets, and verifies that quoted
//! evidence appears in its source.
//!
//! Mirrors the Ruby `cosmos-llm-evaluate` gem. An evaluation answers one
//! question: over these examples, how often does this program get it right?
//! That number is what makes a change to a prompt or a model an experiment
//! rather than a guess.
//!
//! The crate knows nothing about how programs are built. A [`Program`] is any
//! async function from input values to output values, and a [`Metric`] is any
//! function from an example and a prediction to a score, so a predictor, a
//! pipeline, and a lookup table are all evaluated the same way.
//!
//! ## Quick start
//!
//! ```
//! use cosmos_llm_evaluate::{metrics, Evaluator, ProgramError, Values};
//! use cosmos_llm_example::{devset, Example};
//! use serde_json::json;
//!
//! # tokio_test::block_on(async {
//! let jsonl = r#"{"question": "2+2?", "answer": "4"}
//! {"question": "3+3?", "answer": "6"}"#;
//! let examples = devset::read(jsonl, devset::format("jsonl").unwrap(), &["question"]).unwrap();
//!
//! let program = |inputs: Values| async move {
//!     let answer = if inputs["question"] == "2+2?" { "4" } else { "five" };
//!     Ok::<Values, ProgramError>(json!({ "answer": answer }).as_object().unwrap().clone())
//! };
//!
//! let report = Evaluator::new(metrics::f1("answer")).run(&program, &examples).await.unwrap();
//! assert_eq!(report.summary(), "50.0% (1/2 passed)");
//! for row in report.failures(1.0) {
//!     println!("row {}: scored {}", row.index, row.score);
//! }
//! # })
//! ```
//!
//! ## Swapping pieces
//!
//! Metrics are a registry: [`MetricPlugin`] maps a name to a builder taking
//! [`MetricOptions`]. `exact_match`, `f1`, `contains`, `set_overlap`, and
//! `grounded` are built in. Register a new name to add a metric the CLI and
//! config files can select; register a built-in's name to replace it.
//!
//! ## Command line
//!
//! With the `cli` feature this crate builds `cosmos-llm-evaluate`, which
//! scores a predictions file against a devset and checks quotes against a
//! source document. [`cli::main`] is public, so a binary linking your own
//! metrics can reuse the whole command line.

#![warn(missing_docs)]

mod error;
mod evaluator;
pub mod evidence;
mod metric;
pub mod metrics;
mod report;

#[cfg(feature = "cli")]
pub mod cli;

pub use error::EvaluateError;
pub use evaluator::{score_predictions, Evaluator, Program, ProgramError, DEFAULT_CONCURRENCY};
pub use metric::{metric, Metric, MetricOptions, MetricPlugin, Values};
pub use report::{Report, Row, DEFAULT_THRESHOLD};

/// The registry crate, re-exported so plugins can register through it.
pub use cosmos_llm_registry as registry;
