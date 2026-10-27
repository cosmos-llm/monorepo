//! Custom commands (230): `/review`, `/db:migrate`.
//!
//! Two front ends lower into one IR, [`CommandSpec`]:
//!
//! - `.trongo` (preferred): a map, so the prompt can come from
//!   `(read-file "review.prompt.erb")`.
//! - Markdown with frontmatter (compatibility with Claude Code and opencode
//!   command files).
//!
//! ```text
//! ~/.config/cosmos/commands/review.trongo     -> /review      (user)
//! <repo>/.cosmos/commands/review.trongo       -> /review      (project, wins)
//! <repo>/.cosmos/commands/db/migrate.md       -> /db:migrate
//! ```
//!
//! Other sources (a team server, a plugin crate) register a
//! [`CommandSourceReg`] through `cosmos-llm-registry`; their commands rank
//! below the user's own.
//!
//! [`CommandSpec::expand`] turns a command and its arguments into the prompt
//! the model sees: `$ARGUMENTS`, `$1`..`$9`, `@path` inlines a file, and
//! `` !`cmd` `` inlines a shell command's output, but only when the policy
//! engine allows that command without asking.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cosmos_llm_registry::{Meta, Plugin};
use cosmos_llm_tool::ApprovalRequest;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{Layer, LoadOptions};
use crate::defs::{eval_def, listify, parse_frontmatter, read_text, walk_defs, DefKind};
use crate::error::{CoreError, Result};
use crate::eval::{ConfigEvaluator, EvalSnapshot};
use crate::policy::{PolicyEngine, PolicyVerdict};

/// A custom command: the IR both `.trongo` and markdown files lower into.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct CommandSpec {
    /// The name after the slash; directories become `:` (`db:migrate`).
    /// Taken from the file path when the file does not set it.
    pub name: String,
    /// One line for the completion popup.
    pub description: String,
    /// Argument hint for the popup (`[focus]`).
    #[serde(alias = "argument_hint")]
    pub args: Option<String>,
    /// Model for this command's turn only.
    pub model: Option<String>,
    /// Tools the command expects to use. Carried for frontends; the policy,
    /// not this list, decides what runs.
    pub allowed_tools: Vec<String>,
    /// The template. See [`CommandSpec::expand`].
    pub prompt: String,
    /// Which layer defined it, when it came from a file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer: Option<Layer>,
}

impl CommandSpec {
    /// A command with a name and a prompt.
    pub fn new(name: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            prompt: prompt.into(),
            ..Self::default()
        }
    }

    /// Lowers an evaluated value (or frontmatter plus body) into the IR.
    pub fn from_value(value: Value) -> Result<Self> {
        let Value::Object(mut map) = value else {
            return Err(CoreError::Config(
                "a command must evaluate to a map".to_owned(),
            ));
        };
        listify(&mut map, "allowed_tools");
        serde_json::from_value(Value::Object(map))
            .map_err(|e| CoreError::Config(format!("value does not match CommandSpec: {e}")))
    }

    /// Lowers a markdown command file: frontmatter fields, body as the prompt.
    pub fn from_markdown(name: &str, text: &str) -> Result<Self> {
        let (mut map, body) = parse_frontmatter(text);
        map.insert("prompt".to_owned(), Value::String(body));
        let mut spec = Self::from_value(Value::Object(map))?;
        if spec.name.is_empty() {
            spec.name = name.to_owned();
        }
        Ok(spec)
    }

    /// Expands the template for `args`.
    ///
    /// `$ARGUMENTS` is the raw argument string and `$1`..`$9` its
    /// shell-style words (missing ones are empty). Then `@path` tokens are
    /// replaced by the file's contents (workspace files only; a token that is
    /// not a file stays as typed) and `` !`cmd` `` by the command's output.
    pub async fn expand(&self, args: &str, env: &Expansion) -> Expanded {
        let mut out = Expanded::default();
        let text = substitute_args(&self.prompt, args);
        let text = inline_shell(&text, env, &mut out.warnings).await;
        out.text = inline_files(&text, env, &mut out.warnings);
        out
    }
}

/// The result of [`CommandSpec::expand`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Expanded {
    /// The prompt to send.
    pub text: String,
    /// Things the user should know: a `` !`cmd` `` the policy refused.
    pub warnings: Vec<String>,
}

/// What expansion may touch.
#[derive(Clone)]
#[non_exhaustive]
pub struct Expansion {
    /// The workspace; `@path` is relative to it and confined to it.
    pub cwd: PathBuf,
    /// Decides whether a `` !`cmd` `` may run. Without one, none do.
    pub engine: Option<Arc<PolicyEngine>>,
    /// Longest a shell command may run.
    pub shell_timeout: Duration,
    /// Largest file or command output inlined, in bytes.
    pub max_bytes: usize,
}

impl Expansion {
    /// Expansion in `cwd` with no shell allowed.
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            engine: None,
            shell_timeout: Duration::from_secs(30),
            max_bytes: 64 * 1024,
        }
    }

    /// Lets `` !`cmd` `` run what `engine` allows without asking.
    #[must_use]
    pub fn engine(mut self, engine: Arc<PolicyEngine>) -> Self {
        self.engine = Some(engine);
        self
    }
}

impl std::fmt::Debug for Expansion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Expansion")
            .field("cwd", &self.cwd)
            .field("shell", &self.engine.is_some())
            .finish_non_exhaustive()
    }
}

/// Splits `args` into words, honouring single and double quotes.
pub fn split_args(args: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut have = false;
    for c in args.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                have = true;
            }
            (None, c) if c.is_whitespace() => {
                if have || !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                    have = false;
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if have || !cur.is_empty() {
        words.push(cur);
    }
    words
}

fn substitute_args(template: &str, args: &str) -> String {
    let words = split_args(args);
    let mut out = String::with_capacity(template.len() + args.len());
    let mut rest = template;
    while let Some(i) = rest.find('$') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        if let Some(tail) = after.strip_prefix("ARGUMENTS") {
            out.push_str(args.trim());
            rest = tail;
        } else if let Some(d) = after.chars().next().filter(|c| ('1'..='9').contains(c)) {
            let n = d as usize - '1' as usize;
            out.push_str(words.get(n).map_or("", String::as_str));
            rest = &after[1..];
        } else {
            out.push('$');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

fn cap(mut s: String, max: usize) -> String {
    if s.len() > max {
        let mut cut = max;
        while !s.is_char_boundary(cut) {
            cut -= 1;
        }
        s.truncate(cut);
        s.push_str("\n[truncated]");
    }
    s
}

async fn inline_shell(text: &str, env: &Expansion, warnings: &mut Vec<String>) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("!`") {
        let Some(len) = rest[start + 2..].find('`') else {
            break;
        };
        let cmd = &rest[start + 2..start + 2 + len];
        out.push_str(&rest[..start]);
        out.push_str(&run_inline(cmd, env, warnings).await);
        rest = &rest[start + 2 + len + 1..];
    }
    out.push_str(rest);
    out
}

async fn run_inline(cmd: &str, env: &Expansion, warnings: &mut Vec<String>) -> String {
    let refuse = |why: String, warnings: &mut Vec<String>| {
        warnings.push(format!("`{cmd}` was not run: {why}"));
        format!("[command not run: {why}]")
    };
    let Some(engine) = &env.engine else {
        return refuse("no policy engine to check it".into(), warnings);
    };
    let mut req = ApprovalRequest::new(
        "command-expansion",
        "bash",
        serde_json::json!({ "command": cmd }),
    );
    req.hints = cosmos_llm_tool::ToolHints::new().destructive(true);
    match engine.evaluate(&req) {
        PolicyVerdict::Allow => {}
        PolicyVerdict::Deny { reason } => return refuse(reason, warnings),
        PolicyVerdict::Ask { reason } => {
            return refuse(
                format!("it needs approval ({reason}); commands in a prompt are never asked about"),
                warnings,
            )
        }
    }
    let run = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(&env.cwd)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    match tokio::time::timeout(env.shell_timeout, run).await {
        Ok(Ok(o)) => {
            let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
            let err = String::from_utf8_lossy(&o.stderr);
            if !err.trim().is_empty() {
                if !s.is_empty() && !s.ends_with('\n') {
                    s.push('\n');
                }
                s.push_str(&err);
            }
            cap(s.trim_end().to_owned(), env.max_bytes)
        }
        Ok(Err(e)) => refuse(format!("it did not start: {e}"), warnings),
        Err(_) => refuse(format!("it ran past {:?}", env.shell_timeout), warnings),
    }
}

fn inline_files(text: &str, env: &Expansion, warnings: &mut Vec<String>) -> String {
    let root = env.cwd.canonicalize().unwrap_or_else(|_| env.cwd.clone());
    let mut out = String::with_capacity(text.len());
    let mut prev_ws = true;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '@' && prev_ws {
            let tail = &text[i + 1..];
            let end = tail.find(|c: char| c.is_whitespace()).unwrap_or(tail.len());
            let token = tail[..end].trim_end_matches(['.', ',', ';', ':', ')', '!', '?']);
            if !token.is_empty() {
                if let Some(block) = read_inline(&root, token, env.max_bytes, warnings) {
                    out.push_str(&block);
                    let consumed = token.len();
                    while chars.peek().is_some_and(|(j, _)| *j <= i + consumed) {
                        chars.next();
                    }
                    prev_ws = false;
                    continue;
                }
            }
        }
        prev_ws = c.is_whitespace();
        out.push(c);
    }
    out
}

fn read_inline(root: &Path, token: &str, max: usize, warnings: &mut Vec<String>) -> Option<String> {
    let path = root.join(token);
    let canon = path.canonicalize().ok()?;
    if !canon.is_file() {
        return None;
    }
    if !canon.starts_with(root) {
        warnings.push(format!(
            "@{token} is outside the workspace and was not inlined"
        ));
        return None;
    }
    let bytes = std::fs::read(&canon).ok()?;
    let Ok(text) = String::from_utf8(bytes) else {
        warnings.push(format!("@{token} is not text and was not inlined"));
        return None;
    };
    let text = cap(text, max);
    Some(format!("\n<file path=\"{token}\">\n{text}\n</file>\n"))
}

/// A registered source of commands (a team server, a plugin crate), found
/// through `cosmos-llm-registry`.
///
/// ```ignore
/// cosmos_llm_registry::inventory::submit! {
///     CommandSourceReg { meta: Meta::new("team"), load: team_commands }
/// }
/// ```
pub struct CommandSourceReg {
    /// Name, priority and summary.
    pub meta: Meta,
    /// Produces the commands for the workspace at the given path.
    pub load: fn(&Path) -> Vec<CommandSpec>,
}

impl Plugin for CommandSourceReg {
    fn meta(&self) -> &Meta {
        &self.meta
    }
}

cosmos_llm_registry::inventory::collect!(CommandSourceReg);

/// The commands a core offers, by name.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CommandSet {
    specs: BTreeMap<String, CommandSpec>,
}

impl CommandSet {
    /// An empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads registered sources, then the user's directory, then the
    /// project's (later wins). Returns the set and warnings for files that
    /// did not load.
    pub fn load(opts: &LoadOptions) -> (Self, Vec<String>) {
        let mut set = Self::new();
        let mut warnings = Vec::new();
        for source in cosmos_llm_registry::active::<CommandSourceReg>() {
            for spec in (source.load)(&opts.cwd) {
                set.insert(spec);
            }
        }
        let snapshot = opts
            .snapshot
            .clone()
            .unwrap_or_else(|| EvalSnapshot::capture(&opts.cwd));
        let ev = ConfigEvaluator::new(snapshot);
        if let Some(u) = opts.resolved_user_dir() {
            set.load_dir(&u.join("commands"), Layer::User, &ev, &mut warnings);
        }
        set.load_dir(
            &opts.resolved_project_dir().join("commands"),
            Layer::Project,
            &ev,
            &mut warnings,
        );
        (set, warnings)
    }

    /// Adds every command file under `dir`, replacing same-named ones.
    pub fn load_dir(
        &mut self,
        dir: &Path,
        layer: Layer,
        ev: &ConfigEvaluator,
        warnings: &mut Vec<String>,
    ) {
        let mut seen: BTreeMap<String, DefKind> = BTreeMap::new();
        for file in walk_defs(dir) {
            if let Some(DefKind::Trongo) = seen.get(&file.name) {
                warnings.push(format!(
                    "{}: ignored, a .trongo command of the same name wins",
                    file.path.display()
                ));
                continue;
            }
            let loaded = match file.kind {
                DefKind::Trongo => eval_def(ev, &file.path).and_then(|v| {
                    let mut spec = CommandSpec::from_value(v)?;
                    if spec.name.is_empty() {
                        spec.name = file.name.clone();
                    }
                    Ok(spec)
                }),
                DefKind::Markdown => read_text(&file.path)
                    .and_then(|text| CommandSpec::from_markdown(&file.name, &text)),
            };
            match loaded {
                Ok(mut spec) if !spec.prompt.trim().is_empty() => {
                    spec.layer = Some(layer);
                    seen.insert(file.name.clone(), file.kind);
                    self.insert(spec);
                }
                Ok(_) => warnings.push(format!("{}: empty prompt, skipped", file.path.display())),
                Err(e) => warnings.push(format!("{}: {e}", file.path.display())),
            }
        }
    }

    /// Adds a command, replacing one of the same name.
    pub fn insert(&mut self, spec: CommandSpec) {
        self.specs.insert(spec.name.clone(), spec);
    }

    /// A command by name (a leading `/` is ignored).
    pub fn get(&self, name: &str) -> Option<&CommandSpec> {
        self.specs.get(name.trim_start_matches('/'))
    }

    /// All commands, sorted by name.
    pub fn list(&self) -> Vec<CommandSpec> {
        self.specs.values().cloned().collect()
    }

    /// How many commands there are.
    pub fn len(&self) -> usize {
        self.specs.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_split_like_a_shell() {
        assert_eq!(split_args(r#"a "b c" 'd e' f"#), ["a", "b c", "d e", "f"]);
        assert_eq!(split_args("  "), Vec::<String>::new());
        assert_eq!(split_args(r#""" x"#), ["", "x"]);
    }

    #[test]
    fn substitution() {
        assert_eq!(
            substitute_args("do $ARGUMENTS: $1 then $2 and $3 cost $", "x \"y z\""),
            "do x \"y z\": x then y z and  cost $"
        );
    }
}
