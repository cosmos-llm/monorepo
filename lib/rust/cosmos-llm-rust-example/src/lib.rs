//! # cosmos-llm-example
//!
//! Labeled examples and the devsets that hold them.
//!
//! Mirrors the `Example` class of the Ruby `cosmos-llm-predict` gem, split out
//! because three separate crates need it: adapters render examples as
//! demonstrations, predictors return values shaped like them, and evaluators
//! score programs against them. None of those should have to depend on the
//! others to share a row type.
//!
//! ## Quick start
//!
//! ```
//! use cosmos_llm_example::{devset, Example};
//!
//! let jsonl = r#"{"question": "What is 2+2?", "answer": "4"}
//! {"question": "What is 3+3?", "answer": "6"}"#;
//!
//! let examples = devset::read(jsonl, devset::format("jsonl").unwrap(), &["question"]).unwrap();
//! let (train, dev) = devset::split(examples, 0.5, 42).unwrap();
//!
//! for example in &dev {
//!     // What the program receives, and what it is scored against.
//!     let _inputs = example.inputs();
//!     let _labels = example.labels();
//! }
//! # assert_eq!(train.len() + dev.len(), 2);
//! ```
//!
//! ## Swapping pieces
//!
//! Devset file formats are a registry: [`devset::FormatPlugin`] maps a name
//! and file extensions to a [`devset::DevsetFormat`]. `jsonl` and `json` are
//! built in. Register a CSV reader, or replace the JSONL one, without
//! touching this crate.
//!
//! ## Command line
//!
//! With the `cli` feature this crate builds `cosmos-llm-example`, which
//! inspects, splits, converts, and projects devsets — `inputs` emits just the
//! input columns as JSONL, ready to pipe into a predictor.

#![warn(missing_docs)]

pub mod devset;
mod error;
mod example;

#[cfg(feature = "cli")]
pub mod cli;

pub use error::ExampleError;
pub use example::Example;

/// The registry crate, re-exported so plugins can register through it.
pub use cosmos_llm_registry as registry;
