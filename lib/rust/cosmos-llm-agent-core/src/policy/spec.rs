//! The policy IR (150): what config lowers into and the engine reads.
//!
//! Plain serde, no evaluator in sight. A `.trongo` file evaluates to a value of
//! this shape and `serde_json::from_value` does the rest.

use serde::{Deserialize, Serialize};

/// How much the agent may do without asking.
///
/// | Mode | Read-only tools | Edits in workspace | Shell | Outside workspace |
/// |---|---|---|---|---|
/// | `read_only` | allow | deny | known-safe only | deny |
/// | `ask` | allow | ask | known-safe allow, else ask | ask |
/// | `auto_edit` | allow | allow | known-safe allow, else ask | ask |
/// | `full_auto` | allow | allow | allow | ask |
///
/// Config also accepts the hyphenated spellings (`:read-only`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ApprovalMode {
    /// Look, do not touch.
    #[serde(alias = "read-only", alias = "readonly")]
    ReadOnly,
    /// Ask before edits and unknown commands. The default.
    #[default]
    Ask,
    /// Edits inside the workspace are allowed.
    #[serde(alias = "auto-edit")]
    AutoEdit,
    /// Everything inside the rules is allowed. **Not sandboxed.**
    #[serde(alias = "full-auto")]
    FullAuto,
}

impl ApprovalMode {
    /// Orders modes from most to least restrictive (0 is `read_only`).
    pub fn permissiveness(self) -> u8 {
        match self {
            Self::ReadOnly => 0,
            Self::Ask => 1,
            Self::AutoEdit => 2,
            Self::FullAuto => 3,
        }
    }

    /// The more restrictive of two modes.
    pub fn stricter(self, other: Self) -> Self {
        if other.permissiveness() < self.permissiveness() {
            other
        } else {
            self
        }
    }
}

/// One allow or deny rule.
///
/// `tool` names the tool (`"*"` for any). `command`, for shell tools, is a
/// wildcard (`*`, `?`) matched against each simple command of the parsed line,
/// so `cargo test*` matches `cargo test --lib` but never `cargo test && rm x`
/// as a whole. `path` is a glob matched against the call's paths, relative to
/// the workspace root. A rule with neither covers the whole tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Rule {
    /// Tool name, or `*`.
    pub tool: String,
    /// Wildcard over a simple shell command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Glob over the call's paths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl Rule {
    /// A rule covering all of `tool`.
    pub fn tool(tool: impl Into<String>) -> Self {
        Self {
            tool: tool.into(),
            command: None,
            path: None,
        }
    }

    /// Narrows the rule to shell commands matching `pattern`.
    #[must_use]
    pub fn command(mut self, pattern: impl Into<String>) -> Self {
        self.command = Some(pattern.into());
        self
    }

    /// Narrows the rule to paths matching `glob`.
    #[must_use]
    pub fn path(mut self, glob: impl Into<String>) -> Self {
        self.path = Some(glob.into());
        self
    }
}

fn default_shell_tools() -> Vec<String> {
    vec!["bash".into(), "shell".into()]
}

fn default_edit_tools() -> Vec<String> {
    vec!["write".into(), "edit".into(), "multi_edit".into()]
}

/// The permission config: mode plus ordered rules.
///
/// Evaluation order is deny rules, session grants, allow rules, then the mode
/// default. A deny always beats an allow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct PolicySpec {
    /// The default behaviour.
    pub mode: ApprovalMode,
    /// Rules that allow a call without asking.
    pub allow: Vec<Rule>,
    /// Rules that refuse a call.
    pub deny: Vec<Rule>,
    /// Names of tools that run shell commands (their `command` input is
    /// parsed and classified).
    pub shell_tools: Vec<String>,
    /// Names of tools that edit files, used when a tool gives no better hint.
    pub edit_tools: Vec<String>,
}

impl Default for PolicySpec {
    fn default() -> Self {
        Self {
            mode: ApprovalMode::default(),
            allow: Vec::new(),
            deny: Vec::new(),
            shell_tools: default_shell_tools(),
            edit_tools: default_edit_tools(),
        }
    }
}

impl PolicySpec {
    /// A spec in `mode` with no rules.
    pub fn new(mode: ApprovalMode) -> Self {
        Self {
            mode,
            ..Self::default()
        }
    }

    /// Adds an allow rule.
    #[must_use]
    pub fn allow(mut self, rule: Rule) -> Self {
        self.allow.push(rule);
        self
    }

    /// Adds a deny rule.
    #[must_use]
    pub fn deny(mut self, rule: Rule) -> Self {
        self.deny.push(rule);
        self
    }
}
