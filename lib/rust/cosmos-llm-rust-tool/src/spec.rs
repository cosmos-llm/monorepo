//! The data half of a tool: what providers, policies and session logs see.
//!
//! [`ToolSpec`] is the IR (005). [`ToolDefinition`](crate::ToolDefinition) is a
//! `ToolSpec` plus the handler and per-call closures that cannot be data.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::output_limit::OutputLimit;
use crate::parameter::ParameterDef;
use crate::schemas;

/// What a policy may know about a tool call without knowing the tool.
///
/// On a [`ToolSpec`] it is the static default; on an
/// [`ApprovalRequest`](crate::ApprovalRequest) it is the answer for one call,
/// with `paths` filled in from the arguments.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_tool::ToolHints;
///
/// let h = ToolHints::new().destructive(true).paths(["src/lib.rs"]);
/// assert!(h.destructive && !h.read_only);
/// assert_eq!(h.paths.len(), 1);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(default)]
pub struct ToolHints {
    /// The call only reads (read, glob, grep, list).
    pub read_only: bool,
    /// The call can destroy or overwrite data (write, edit, shell).
    pub destructive: bool,
    /// Paths the call touches, extracted from its arguments.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<PathBuf>,
}

impl ToolHints {
    /// No claims: neither read-only nor destructive, no paths.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets [`ToolHints::read_only`].
    #[must_use]
    pub fn read_only(mut self, yes: bool) -> Self {
        self.read_only = yes;
        self
    }

    /// Sets [`ToolHints::destructive`].
    #[must_use]
    pub fn destructive(mut self, yes: bool) -> Self {
        self.destructive = yes;
        self
    }

    /// Sets [`ToolHints::paths`].
    #[must_use]
    pub fn paths<I, P>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        self.paths = paths.into_iter().map(Into::into).collect();
        self
    }
}

/// A tool's name, description, parameters and static metadata.
///
/// Plain data: it loads from JSON or a `.trongo` file, renders to any
/// provider's schema dialect, and travels through logs. Attach a handler with
/// [`ToolDefinition::from_spec`](crate::ToolDefinition::from_spec).
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_tool::{ParameterType, ToolDefinition};
///
/// let tool = ToolDefinition::new("search")
///     .param("q", ParameterType::String, true, "query");
/// let spec = tool.spec();
/// assert_eq!(spec.name, "search");
/// assert_eq!(spec.to_anthropic_schema()["name"], "search");
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ToolSpec {
    /// Name used for registration and in tool calls.
    pub name: String,
    /// Description shown to the model.
    #[serde(default)]
    pub description: String,
    /// Parameter specifications.
    #[serde(default)]
    pub parameters: Vec<ParameterDef>,
    /// Static policy hints; per-call `paths` are not set here.
    #[serde(default)]
    pub hints: ToolHints,
    /// Wall-clock limit for one execution, in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// Per-tool cap on what a result adds to the model's context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_limit: Option<OutputLimit>,
}

impl ToolSpec {
    /// A spec with just a name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            parameters: Vec::new(),
            hints: ToolHints::default(),
            timeout_ms: None,
            output_limit: None,
        }
    }

    /// The timeout as a [`Duration`].
    pub fn timeout(&self) -> Option<Duration> {
        self.timeout_ms.map(Duration::from_millis)
    }

    /// Renders the OpenAI function-calling schema.
    pub fn to_openai_schema(&self) -> serde_json::Value {
        schemas::openai_parts(&self.name, &self.description, &self.parameters)
    }

    /// Renders the Anthropic tool schema.
    pub fn to_anthropic_schema(&self) -> serde_json::Value {
        schemas::anthropic_parts(&self.name, &self.description, &self.parameters)
    }

    /// Renders a JSON Schema for the parameters.
    pub fn to_json_schema(&self) -> serde_json::Value {
        schemas::json_parts(&self.name, &self.description, &self.parameters)
    }
}
