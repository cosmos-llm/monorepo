//! The IR types of the shell tool.
//!
//! [`ShellCommandIr`] is the parsed command tree the policy engine classifies
//! (150); [`ShellRun`] is what the UI gets in `ToolOutcome::display`. Both are
//! plain serde: tagged enums, `snake_case`, nothing parser-specific.

use serde::{Deserialize, Serialize};

/// A parsed shell command line.
///
/// Produced by [`parse_shell`](super::parse_shell); consumed by the policy,
/// which classifies the tree and never the raw text. The tree is
/// deliberately small: simple commands, the operators that join them, and
/// [`Opaque`](Self::Opaque) for everything the parser does not model, which a
/// policy must treat as "ask".
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_coding_tools::shell::{parse_shell, ShellCommandIr};
///
/// let ir = parse_shell("cargo test && rm -rf ~").unwrap();
/// let argv0: Vec<_> = ir
///     .simple_commands()
///     .iter()
///     .map(|c| c.argv[0].text.clone())
///     .collect();
/// assert_eq!(argv0, ["cargo", "rm"]);
/// assert!(matches!(ir, ShellCommandIr::Chain { .. }));
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ShellCommandIr {
    /// One command: `VAR=1 git commit -m x > log`.
    Simple(SimpleCommand),
    /// `a | b | c`, optionally negated with `!`.
    Pipeline {
        /// The stages, in order.
        commands: Vec<ShellCommandIr>,
        /// A leading `!`.
        #[serde(default)]
        negated: bool,
    },
    /// `a && b || c`: left to right, same precedence, as in the shell.
    Chain {
        /// The first command.
        first: Box<ShellCommandIr>,
        /// Each following command and the operator before it.
        rest: Vec<ChainLink>,
    },
    /// Commands separated by `;` or newlines. Empty for an empty line.
    Sequence {
        /// The commands, in order.
        items: Vec<ShellCommandIr>,
    },
    /// `command &`.
    Background {
        /// The command run in the background.
        command: Box<ShellCommandIr>,
    },
    /// `( ... )`.
    Subshell {
        /// The body.
        command: Box<ShellCommandIr>,
        /// Redirections applied to the whole body.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        redirects: Vec<Redirect>,
    },
    /// `{ ...; }`.
    Group {
        /// The body.
        command: Box<ShellCommandIr>,
        /// Redirections applied to the whole body.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        redirects: Vec<Redirect>,
    },
    /// A construct the parser does not model (`if`, `for`, `while`, `case`,
    /// `[[ ]]`, functions, arithmetic commands). `source` is the text from
    /// that point to the end of the input, so nothing after it is hidden.
    /// A policy must not allow a tree that contains one.
    Opaque {
        /// What was not understood.
        reason: String,
        /// The unparsed text.
        source: String,
    },
}

/// A command joined to the previous one by `&&` or `||`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ChainLink {
    /// The operator before `command`.
    pub op: ChainOp,
    /// The command.
    pub command: ShellCommandIr,
}

/// `&&` or `||`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum ChainOp {
    /// `&&`: run the next only if this succeeded.
    And,
    /// `||`: run the next only if this failed.
    Or,
}

/// A command with its words, assignments and redirections.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SimpleCommand {
    /// Leading `NAME=value` words.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assignments: Vec<Assignment>,
    /// The command name and its arguments. Empty for a bare assignment.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub argv: Vec<Word>,
    /// Redirections, in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub redirects: Vec<Redirect>,
}

/// `NAME=value`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Assignment {
    /// The variable name (with a trailing `+` for `+=`).
    pub name: String,
    /// The value.
    pub value: Word,
}

/// One shell word, after quote removal but before expansion.
///
/// `text` is what the word looks like with quotes and escapes resolved;
/// expansions stay in their source form (`"$HOME/x"` has text `$HOME/x`). Only
/// when [`is_literal`](Self::is_literal) is true is `text` the value the
/// command will see.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Word {
    /// The word with quotes removed.
    pub text: String,
    /// An unquoted `*`, `?`, `[` or `{` is present: the shell may expand the
    /// word to other names.
    #[serde(default, skip_serializing_if = "is_false")]
    pub glob: bool,
    /// Expansions inside the word, in order. Substitutions carry their parsed
    /// command, so a policy can classify what they would run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expansions: Vec<Expansion>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Word {
    /// A word with no expansions.
    pub fn literal(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            glob: false,
            expansions: Vec::new(),
        }
    }

    /// `true` if `text` is exactly what the command receives: no expansions
    /// and no globbing.
    pub fn is_literal(&self) -> bool {
        !self.glob && self.expansions.is_empty()
    }

    /// The text, if [`is_literal`](Self::is_literal).
    pub fn as_literal(&self) -> Option<&str> {
        self.is_literal().then_some(self.text.as_str())
    }
}

/// Something the shell expands inside a word.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Expansion {
    /// `$(...)` or backticks: runs a command.
    CommandSubstitution {
        /// The command inside.
        command: Box<ShellCommandIr>,
    },
    /// `<(...)` or `>(...)`: runs a command.
    ProcessSubstitution {
        /// Which way the data flows.
        direction: ProcessDirection,
        /// The command inside.
        command: Box<ShellCommandIr>,
    },
    /// `$NAME` or `${...}`; for the braced form, the text between the braces.
    Variable {
        /// The variable or expression.
        name: String,
    },
    /// `$(( ... ))`.
    Arithmetic {
        /// The expression text.
        source: String,
    },
    /// A leading `~`.
    Tilde,
}

/// Direction of a process substitution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum ProcessDirection {
    /// `<(cmd)`.
    Input,
    /// `>(cmd)`.
    Output,
}

/// A redirection such as `2>&1`, `> out.txt` or `<<EOF`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Redirect {
    /// The file descriptor written to, if given (`2` in `2>x`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fd: Option<u32>,
    /// The operator.
    pub op: RedirectOp,
    /// The file, descriptor, here-string word or here-doc delimiter.
    pub target: Word,
    /// The body of a here-doc. Its expansions run unless the delimiter was
    /// quoted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Word>,
}

/// A redirection operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum RedirectOp {
    /// `<`.
    Read,
    /// `>`.
    Write,
    /// `>>`.
    Append,
    /// `<>`.
    ReadWrite,
    /// `>|`.
    Clobber,
    /// `<&`.
    DupIn,
    /// `>&`.
    DupOut,
    /// `<<` or `<<-`.
    HereDoc,
    /// `<<<`.
    HereString,
    /// `&>`.
    WriteBoth,
    /// `&>>`.
    AppendBoth,
}

impl RedirectOp {
    /// `true` if the operator can create or modify a file.
    pub fn writes(self) -> bool {
        matches!(
            self,
            Self::Write
                | Self::Append
                | Self::ReadWrite
                | Self::Clobber
                | Self::WriteBoth
                | Self::AppendBoth
                | Self::DupOut
        )
    }
}

impl ShellCommandIr {
    /// Every simple command in the tree, in source order, including those
    /// inside subshells, groups and substitutions.
    pub fn simple_commands(&self) -> Vec<&SimpleCommand> {
        let mut out = Vec::new();
        self.walk(&mut |n| {
            if let ShellCommandIr::Simple(c) = n {
                out.push(c);
            }
        });
        out
    }

    /// `true` if any node is [`Opaque`](Self::Opaque).
    pub fn has_opaque(&self) -> bool {
        let mut found = false;
        self.walk(&mut |n| found |= matches!(n, ShellCommandIr::Opaque { .. }));
        found
    }

    /// `true` if any word runs a command: `$(...)`, backticks or `<(...)`.
    pub fn has_substitution(&self) -> bool {
        let mut found = false;
        self.walk_words(&mut |w| {
            found |= w.expansions.iter().any(|e| {
                matches!(
                    e,
                    Expansion::CommandSubstitution { .. } | Expansion::ProcessSubstitution { .. }
                )
            });
        });
        found
    }

    /// Calls `f` on this node and every node below it, descending into
    /// substitutions.
    pub fn walk<'a>(&'a self, f: &mut dyn FnMut(&'a ShellCommandIr)) {
        f(self);
        match self {
            Self::Simple(c) => {
                for w in c
                    .argv
                    .iter()
                    .chain(c.assignments.iter().map(|a| &a.value))
                    .chain(redirect_words(&c.redirects))
                {
                    walk_word(w, f);
                }
            }
            Self::Pipeline { commands, .. } => commands.iter().for_each(|c| c.walk(f)),
            Self::Chain { first, rest } => {
                first.walk(f);
                rest.iter().for_each(|l| l.command.walk(f));
            }
            Self::Sequence { items } => items.iter().for_each(|c| c.walk(f)),
            Self::Background { command } => command.walk(f),
            Self::Subshell { command, redirects } | Self::Group { command, redirects } => {
                command.walk(f);
                for w in redirect_words(redirects) {
                    walk_word(w, f);
                }
            }
            Self::Opaque { .. } => {}
        }
    }

    /// Calls `f` on every word in the tree.
    pub fn walk_words<'a>(&'a self, f: &mut dyn FnMut(&'a Word)) {
        let mut nodes = Vec::new();
        self.walk(&mut |n| nodes.push(n));
        for n in nodes {
            match n {
                Self::Simple(c) => {
                    for w in c
                        .argv
                        .iter()
                        .chain(c.assignments.iter().map(|a| &a.value))
                        .chain(redirect_words(&c.redirects))
                    {
                        f(w);
                    }
                }
                Self::Subshell { redirects, .. } | Self::Group { redirects, .. } => {
                    redirects_words(redirects, f)
                }
                _ => {}
            }
        }
    }
}

fn redirects_words<'a>(r: &'a [Redirect], f: &mut dyn FnMut(&'a Word)) {
    redirect_words(r).for_each(f);
}

fn redirect_words(r: &[Redirect]) -> impl Iterator<Item = &Word> {
    r.iter()
        .flat_map(|r| std::iter::once(&r.target).chain(r.body.as_ref()))
}

fn walk_word<'a>(w: &'a Word, f: &mut dyn FnMut(&'a ShellCommandIr)) {
    for e in &w.expansions {
        match e {
            Expansion::CommandSubstitution { command }
            | Expansion::ProcessSubstitution { command, .. } => command.walk(f),
            _ => {}
        }
    }
}

/// What the UI gets in `ToolOutcome::display` for a `bash` call.
///
/// The model sees only the text; this carries the same facts as data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ShellRun {
    /// The command line as given.
    pub command: String,
    /// The model's short label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Directory the command ran in.
    pub cwd: String,
    /// How the run ended.
    pub status: ShellStatus,
    /// Exit code, when the process exited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Signal that ended the process, if one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<i32>,
    /// Wall time in milliseconds.
    pub duration_ms: u64,
    /// Job id, for a background run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    /// The working directory for the next call, when this one changed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_cwd: Option<String>,
    /// Bytes of output captured.
    pub output_bytes: usize,
    /// Capture hit its memory cap and dropped the rest.
    #[serde(default, skip_serializing_if = "is_false")]
    pub capture_truncated: bool,
}

/// How a shell run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum ShellStatus {
    /// The process exited (any code).
    Exited,
    /// The timeout fired and the process group was killed.
    TimedOut,
    /// The user interrupted and the process group was killed.
    Interrupted,
    /// The shell could not be started.
    SpawnFailed,
    /// Started as a background job.
    Background,
}
