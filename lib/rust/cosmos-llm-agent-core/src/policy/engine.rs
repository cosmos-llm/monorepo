//! The policy engine (150): `(mode, rules, request) -> verdict`.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use cosmos_llm_tool::{ApprovalRequest, ApprovalScope, Approver, Decision};
use futures_util::future::BoxFuture;
use globset::GlobBuilder;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::shell::{is_known_safe, wildcard_match, IrShellParser, ShellParse, ShellParser};
use super::spec::{ApprovalMode, PolicySpec, Rule};

/// What the engine decided about one call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum PolicyVerdict {
    /// Run it.
    Allow,
    /// Refuse it. The reason is shown to the model.
    Deny {
        /// Why, in words the model can act on.
        reason: String,
    },
    /// Put the question to the user.
    Ask {
        /// Why the policy could not decide on its own.
        reason: String,
    },
}

impl PolicyVerdict {
    fn deny(reason: impl Into<String>) -> Self {
        Self::Deny {
            reason: reason.into(),
        }
    }

    fn ask(reason: impl Into<String>) -> Self {
        Self::Ask {
            reason: reason.into(),
        }
    }
}

/// How a call is treated by the mode table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    ReadOnly,
    Edit,
    Shell,
    Other,
}

/// Evaluates [`PolicySpec`] against approval requests.
///
/// Holds the live pieces the IR cannot: the workspace roots, session grants,
/// and the shell parser.
pub struct PolicyEngine {
    spec: Mutex<PolicySpec>,
    cwd: PathBuf,
    roots: Mutex<Vec<PathBuf>>,
    grants: Mutex<Vec<ApprovalScope>>,
    parser: Arc<dyn ShellParser>,
    remember_dir: Option<PathBuf>,
    remember_flags: Mutex<HashSet<String>>,
}

impl std::fmt::Debug for PolicyEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PolicyEngine")
            .field("cwd", &self.cwd)
            .field("mode", &self.mode())
            .finish_non_exhaustive()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Resolves `.`/`..` lexically, then follows symlinks on the longest existing
/// prefix, so a link out of the workspace is seen for what it is.
fn normalize(cwd: &Path, path: &Path) -> PathBuf {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let mut out = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    let mut existing = out.clone();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(canon) = existing.canonicalize() {
            let mut full = canon;
            for part in tail.iter().rev() {
                full.push(part);
            }
            return full;
        }
        match (
            existing.file_name().map(|n| n.to_os_string()),
            existing.parent(),
        ) {
            (Some(name), Some(parent)) => {
                tail.push(name);
                existing = parent.to_path_buf();
            }
            _ => return out,
        }
    }
}

impl PolicyEngine {
    /// An engine for the workspace at `cwd` with the default shell parser.
    pub fn new(spec: PolicySpec, cwd: impl AsRef<Path>) -> Self {
        let cwd = cwd.as_ref();
        let cwd = normalize(Path::new("/"), cwd);
        Self {
            spec: Mutex::new(spec),
            roots: Mutex::new(vec![cwd.clone()]),
            cwd,
            grants: Mutex::new(Vec::new()),
            parser: Arc::new(IrShellParser),
            remember_dir: None,
            remember_flags: Mutex::new(HashSet::new()),
        }
    }

    /// A copy of this engine whose mode is at most `mode` (never more
    /// permissive than the current one): same rules, roots, grants and parser,
    /// no remembered-grant directory. Subagents use it to hold a child to a
    /// stricter mode than its parent (240).
    pub fn derive(&self, mode: ApprovalMode) -> PolicyEngine {
        let mut spec = self.spec();
        spec.mode = spec.mode.stricter(mode);
        PolicyEngine {
            spec: Mutex::new(spec),
            cwd: self.cwd.clone(),
            roots: Mutex::new(self.roots()),
            grants: Mutex::new(self.grants()),
            parser: Arc::clone(&self.parser),
            remember_dir: None,
            remember_flags: Mutex::new(HashSet::new()),
        }
    }

    /// Swaps the shell parser.
    #[must_use]
    pub fn with_parser(mut self, parser: Arc<dyn ShellParser>) -> Self {
        self.parser = parser;
        self
    }

    /// Adds a directory that counts as inside the workspace.
    #[must_use]
    pub fn with_root(self, dir: impl AsRef<Path>) -> Self {
        self.add_root(dir.as_ref());
        self
    }

    /// Where "always, and remember" writes `permissions.trongo`
    /// (`<repo>/.cosmos`).
    #[must_use]
    pub fn with_remember_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.remember_dir = Some(dir.into());
        self
    }

    /// The directory remembered grants are written to, if any.
    pub fn remember_dir(&self) -> Option<&Path> {
        self.remember_dir.as_deref()
    }

    /// The current mode.
    pub fn mode(&self) -> ApprovalMode {
        lock(&self.spec).mode
    }

    /// Changes the mode; takes effect on the next call.
    pub fn set_mode(&self, mode: ApprovalMode) {
        lock(&self.spec).mode = mode;
    }

    /// A copy of the spec as it stands.
    pub fn spec(&self) -> PolicySpec {
        lock(&self.spec).clone()
    }

    /// The workspace roots, including directories added by approvals.
    pub fn roots(&self) -> Vec<PathBuf> {
        lock(&self.roots).clone()
    }

    /// Adds `dir` to the roots (a file's parent is used).
    pub fn add_root(&self, dir: &Path) {
        let abs = normalize(&self.cwd, dir);
        let dir = if abs.is_file() {
            abs.parent().map(Path::to_path_buf).unwrap_or(abs)
        } else {
            abs
        };
        let mut roots = lock(&self.roots);
        if !roots.contains(&dir) {
            roots.push(dir);
        }
    }

    /// Records a session grant from an `AllowAlways` decision.
    pub fn add_grant(&self, scope: ApprovalScope) {
        let mut grants = lock(&self.grants);
        if !grants.contains(&scope) {
            grants.push(scope);
        }
    }

    /// The session grants so far.
    pub fn grants(&self) -> Vec<ApprovalScope> {
        lock(&self.grants).clone()
    }

    /// Marks `call_id` so its `AllowAlways` is also written to
    /// `permissions.trongo`.
    pub fn flag_remember(&self, call_id: &str) {
        lock(&self.remember_flags).insert(call_id.to_owned());
    }

    fn take_remember(&self, call_id: &str) -> bool {
        lock(&self.remember_flags).remove(call_id)
    }

    /// The rule a scope would be remembered as, when it can be expressed.
    ///
    /// `Input` scopes on a shell tool become an exact-command rule; other
    /// `Input` scopes have no rule form and are session-only.
    pub fn scope_to_rule(&self, scope: &ApprovalScope, req: &ApprovalRequest) -> Option<Rule> {
        match scope {
            ApprovalScope::Tool { name } => Some(Rule::tool(name.clone())),
            ApprovalScope::Path { name, prefix } => {
                Some(Rule::tool(name.clone()).path(format!("{}/**", prefix.trim_end_matches('/'))))
            }
            ApprovalScope::Input { name, .. } => {
                let is_shell = lock(&self.spec).shell_tools.contains(name);
                let cmd = req.input.get("command").and_then(Value::as_str)?;
                is_shell.then(|| Rule::tool(name.clone()).command(cmd.to_owned()))
            }
            _ => None,
        }
    }

    fn rel_for_match(&self, path: &Path) -> String {
        let abs = normalize(&self.cwd, path);
        for root in lock(&self.roots).iter() {
            if let Ok(rel) = abs.strip_prefix(root) {
                return rel.to_string_lossy().into_owned();
            }
        }
        abs.to_string_lossy().into_owned()
    }

    fn path_matches(&self, glob: &str, path: &Path) -> bool {
        let Ok(g) = GlobBuilder::new(glob).literal_separator(true).build() else {
            return false;
        };
        let m = g.compile_matcher();
        let abs = normalize(&self.cwd, path);
        m.is_match(self.rel_for_match(path)) || m.is_match(&abs)
    }

    fn outside(&self, path: &Path) -> bool {
        let abs = normalize(&self.cwd, path);
        !lock(&self.roots).iter().any(|r| abs.starts_with(r))
    }

    fn classify(&self, req: &ApprovalRequest, spec: &PolicySpec) -> (Class, Option<ShellParse>) {
        if spec.shell_tools.contains(&req.tool) {
            let parse = match req.input.get("command").and_then(Value::as_str) {
                Some(cmd) => self.parser.parse(cmd),
                None => ShellParse::opaque("the call has no command text"),
            };
            return (Class::Shell, Some(parse));
        }
        if req.hints.read_only {
            return (Class::ReadOnly, None);
        }
        if !req.hints.paths.is_empty() || spec.edit_tools.contains(&req.tool) {
            return (Class::Edit, None);
        }
        (Class::Other, None)
    }

    /// The paths a call touches: hinted paths, or a shell line's redirect
    /// targets.
    fn touched(req: &ApprovalRequest, parse: Option<&ShellParse>) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = req.hints.paths.clone();
        if let Some(p) = parse {
            for c in &p.commands {
                for r in &c.redirects {
                    if r.target != "/dev/null" {
                        paths.push(PathBuf::from(&r.target));
                    }
                }
            }
        }
        paths
    }

    fn rule_tool_matches(rule: &Rule, tool: &str) -> bool {
        rule.tool == "*" || rule.tool == tool
    }

    /// Whether a deny rule applies: any command, any path.
    fn deny_matches(
        &self,
        rule: &Rule,
        req: &ApprovalRequest,
        parse: Option<&ShellParse>,
        paths: &[PathBuf],
    ) -> bool {
        if !Self::rule_tool_matches(rule, &req.tool) {
            return false;
        }
        if let Some(pattern) = &rule.command {
            let Some(parse) = parse else { return false };
            let raw = req
                .input
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or("");
            let hit = parse
                .commands
                .iter()
                .any(|c| wildcard_match(pattern, &c.text()))
                || wildcard_match(pattern, raw.trim());
            if !hit {
                return false;
            }
        }
        if let Some(glob) = &rule.path {
            if !paths.iter().any(|p| self.path_matches(glob, p)) {
                return false;
            }
        }
        true
    }

    /// Whether allow rules cover the call. Every shell command, or every
    /// path, must be covered.
    fn allow_covers(
        &self,
        spec: &PolicySpec,
        req: &ApprovalRequest,
        parse: Option<&ShellParse>,
        paths: &[PathBuf],
        outside: bool,
    ) -> bool {
        let rules: Vec<&Rule> = spec
            .allow
            .iter()
            .filter(|r| Self::rule_tool_matches(r, &req.tool))
            .collect();
        if rules.is_empty() {
            return false;
        }
        if let Some(parse) = parse {
            if parse.opaque.is_some() || parse.commands.is_empty() || outside {
                return false;
            }
            return parse.commands.iter().all(|c| {
                if c.redirects
                    .iter()
                    .any(|r| r.writes() && r.target != "/dev/null")
                {
                    // Output redirection is a file edit the command rule did
                    // not mention.
                    return false;
                }
                let text = c.text();
                rules.iter().any(|r| match (&r.command, &r.path) {
                    (Some(p), None) => wildcard_match(p, &text),
                    (None, None) => true,
                    _ => false,
                })
            });
        }
        rules.iter().any(|r| {
            if r.command.is_some() {
                return false;
            }
            match &r.path {
                Some(glob) => !paths.is_empty() && paths.iter().all(|p| self.path_matches(glob, p)),
                None => !outside,
            }
        })
    }

    /// Decides one call.
    pub fn evaluate(&self, req: &ApprovalRequest) -> PolicyVerdict {
        let spec = lock(&self.spec).clone();
        let (class, parse) = self.classify(req, &spec);
        let paths = Self::touched(req, parse.as_ref());
        let outside_paths: Vec<&PathBuf> = paths.iter().filter(|p| self.outside(p)).collect();
        let outside = !outside_paths.is_empty();

        for rule in &spec.deny {
            if self.deny_matches(rule, req, parse.as_ref(), &paths) {
                return PolicyVerdict::deny(format!(
                    "blocked by a deny rule ({}). Do not retry this; ask the user to change the rule if it is needed.",
                    describe(rule)
                ));
            }
        }

        // Deny rules only see commands the parser can read. A line it cannot
        // read (`bash -c '...'`, `eval`, `sudo`, substitution) may hide a
        // command a deny rule names, so neither a grant, an allow rule nor
        // `full_auto` may wave it through: it goes to the user, or is refused
        // when there is nobody to ask.
        if class == Class::Shell {
            if let Some(why) = parse.as_ref().and_then(|p| p.opaque.as_deref()) {
                if spec
                    .deny
                    .iter()
                    .any(|r| Self::rule_tool_matches(r, &req.tool))
                {
                    return match spec.mode {
                        ApprovalMode::ReadOnly => PolicyVerdict::deny(format!(
                            "the command cannot be checked ({why}), and a deny rule applies to {}.",
                            req.tool
                        )),
                        _ => PolicyVerdict::ask(format!(
                            "the command cannot be checked ({why}), and a deny rule applies to {}",
                            req.tool
                        )),
                    };
                }
            }
        }

        let granted = lock(&self.grants)
            .iter()
            .any(|s| s.covers(&req.tool, &req.input, &req.hints));
        if granted || self.allow_covers(&spec, req, parse.as_ref(), &paths, outside) {
            return PolicyVerdict::Allow;
        }

        if outside {
            let first = outside_paths[0].display();
            return match spec.mode {
                ApprovalMode::ReadOnly => PolicyVerdict::deny(format!(
                    "{first} is outside the workspace, which read-only mode does not allow."
                )),
                _ => PolicyVerdict::ask(format!("{first} is outside the workspace")),
            };
        }

        let mode = spec.mode;
        match class {
            Class::ReadOnly => PolicyVerdict::Allow,
            Class::Edit => match mode {
                ApprovalMode::ReadOnly => PolicyVerdict::deny(
                    "read-only mode does not allow file edits. Describe the change instead of making it.",
                ),
                ApprovalMode::Ask => PolicyVerdict::ask("edits need approval in ask mode"),
                _ => PolicyVerdict::Allow,
            },
            Class::Shell => {
                let parse = parse.unwrap_or_default();
                let opaque = parse.opaque.clone();
                let safe = opaque.is_none()
                    && !parse.commands.is_empty()
                    && parse.commands.iter().all(is_known_safe);
                if safe || mode == ApprovalMode::FullAuto {
                    return PolicyVerdict::Allow;
                }
                let why = match opaque {
                    Some(o) => format!("the command cannot be checked ({o})"),
                    None => "the command is not on the known-safe list".to_owned(),
                };
                match mode {
                    ApprovalMode::ReadOnly => PolicyVerdict::deny(format!(
                        "read-only mode only runs known-safe commands (ls, cat, git status, ...); {why}."
                    )),
                    _ => PolicyVerdict::ask(why),
                }
            }
            Class::Other => match mode {
                ApprovalMode::FullAuto => PolicyVerdict::Allow,
                ApprovalMode::ReadOnly => PolicyVerdict::deny(format!(
                    "read-only mode does not allow the {} tool.",
                    req.tool
                )),
                _ => PolicyVerdict::ask(format!("{} needs approval", req.tool)),
            },
        }
    }

    /// Updates engine state from an answered question: grants, widened roots,
    /// and the optional write to `permissions.trongo`. Returns the decision
    /// to hand the loop (an `AllowAlways` becomes `Allow`, because the engine
    /// holds the grant itself and a loop-held grant would skip deny rules).
    pub fn observe(&self, req: &ApprovalRequest, decision: Decision) -> (Decision, Option<String>) {
        let remember = self.take_remember(&req.call_id);
        let mut note = None;
        let allowed = matches!(decision, Decision::Allow | Decision::AllowAlways { .. });
        if allowed {
            let parse = {
                let spec = lock(&self.spec);
                self.classify(req, &spec).1
            };
            for p in Self::touched(req, parse.as_ref()) {
                if self.outside(&p) {
                    self.add_root(&p);
                }
            }
        }
        match decision {
            Decision::AllowAlways { scope } => {
                if remember {
                    note = match (self.scope_to_rule(&scope, req), &self.remember_dir) {
                        (Some(rule), Some(dir)) => {
                            match crate::config::append_permission_rule(dir, &rule) {
                                Ok(path) => Some(format!("remembered in {}", path.display())),
                                Err(e) => Some(format!("could not remember the grant: {e}")),
                            }
                        }
                        (None, _) => Some(
                            "this grant has no rule form and lasts for the session only".to_owned(),
                        ),
                        (_, None) => {
                            Some("no project config directory; grant is session-only".to_owned())
                        }
                    };
                }
                self.add_grant(scope);
                (Decision::Allow, note)
            }
            other => (other, None),
        }
    }
}

fn describe(rule: &Rule) -> String {
    let mut s = rule.tool.clone();
    if let Some(c) = &rule.command {
        s.push_str(&format!(" command {c:?}"));
    }
    if let Some(p) = &rule.path {
        s.push_str(&format!(" path {p:?}"));
    }
    s
}

/// An [`Approver`] that runs the policy first and asks `next` only when the
/// policy says ask.
///
/// This is the head of the approver chain: `PolicyApprover` → interactive
/// approver. Allow and deny verdicts never reach the user.
pub struct PolicyApprover {
    engine: Arc<PolicyEngine>,
    next: Arc<dyn Approver>,
    notes: Option<tokio::sync::mpsc::UnboundedSender<String>>,
}

impl PolicyApprover {
    /// Chains `engine` in front of `next`.
    pub fn new(engine: Arc<PolicyEngine>, next: Arc<dyn Approver>) -> Self {
        Self {
            engine,
            next,
            notes: None,
        }
    }

    /// Sends human-readable notes (such as "remembered in ...") to `tx`.
    #[must_use]
    pub fn with_notes(mut self, tx: tokio::sync::mpsc::UnboundedSender<String>) -> Self {
        self.notes = Some(tx);
        self
    }

    /// The engine behind the chain.
    pub fn engine(&self) -> &Arc<PolicyEngine> {
        &self.engine
    }
}

impl Approver for PolicyApprover {
    fn check<'a>(&'a self, req: &'a ApprovalRequest) -> BoxFuture<'a, Decision> {
        Box::pin(async move {
            match self.engine.evaluate(req) {
                PolicyVerdict::Allow => Decision::Allow,
                PolicyVerdict::Deny { reason } => Decision::deny(reason),
                PolicyVerdict::Ask { .. } => {
                    let answer = self.next.check(req).await;
                    let (decision, note) = self.engine.observe(req, answer);
                    if let (Some(note), Some(tx)) = (note, &self.notes) {
                        let _ = tx.send(note);
                    }
                    decision
                }
            }
        })
    }
}
