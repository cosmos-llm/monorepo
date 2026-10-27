use std::fmt;
use std::path::PathBuf;

use thiserror::Error;

/// Errors produced while building, reading, or writing examples.
#[derive(Debug, Clone, PartialEq, Error)]
#[non_exhaustive]
pub enum ExampleError {
    /// A row was valid JSON but not an object.
    #[error("{}expected a JSON object, found {found}", LinePrefix(*.line))]
    NotObject {
        /// The 1-based line or row number, when known.
        line: Option<usize>,
        /// What was found instead, e.g. `"an array"`.
        found: &'static str,
    },

    /// A row was not valid JSON.
    #[error("{}{message}", LinePrefix(*.line))]
    Parse {
        /// The 1-based line or row number, when known.
        line: Option<usize>,
        /// The parser's complaint.
        message: String,
    },

    /// A file could not be read or written.
    #[error("{}: {message}", path.display())]
    Io {
        /// The file involved.
        path: PathBuf,
        /// What went wrong.
        message: String,
    },

    /// An argument was out of range, such as a split fraction above 1.
    #[error("{0}")]
    Config(String),

    /// A named devset format was not found in the registry.
    #[error(transparent)]
    NotFound(#[from] cosmos_llm_registry::NotFound),
}

struct LinePrefix(Option<usize>);

impl fmt::Display for LinePrefix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(line) => write!(f, "line {line}: "),
            None => Ok(()),
        }
    }
}
