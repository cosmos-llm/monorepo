//! Shell command classification (150).
//!
//! String-prefix matching on `cargo test && rm -rf ~` gets fooled, so the
//! policy never looks at raw text. A [`ShellParser`] lowers a command line into
//! a [`ShellParse`]: simple commands (argv, redirections) in order, or a note
//! that the line holds something the parser cannot reason about. The engine
//! classifies that IR.
//!
//! The parser is a trait so it can be swapped. [`IrShellParser`] lowers
//! `cosmos_llm_coding_tools::ShellCommandIr` into this module's flat view
//! (simple commands plus an optional "opaque" reason); the engine never sees
//! the parser's own tree.

use cosmos_llm_coding_tools::shell::RedirectOp;
use cosmos_llm_coding_tools::{parse_shell, ShellCommandIr};
use serde::{Deserialize, Serialize};

/// A redirection on a simple command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Redirect {
    /// The operator: `>`, `>>`, `>|`, `&>`, `&>>` or `<`.
    pub op: String,
    /// The file the operator names.
    pub target: String,
}

impl Redirect {
    /// Whether this redirection writes to its target.
    pub fn writes(&self) -> bool {
        self.op != "<"
    }
}

/// One simple command: a program, its arguments and its redirections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SimpleCommand {
    /// The program followed by its arguments, quotes removed.
    pub argv: Vec<String>,
    /// Redirections, in source order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub redirects: Vec<Redirect>,
}

impl SimpleCommand {
    /// A command from words, with no redirections.
    pub fn new<I, S>(argv: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            argv: argv.into_iter().map(Into::into).collect(),
            redirects: Vec::new(),
        }
    }

    /// The words joined with single spaces, which is what command rules match.
    pub fn text(&self) -> String {
        self.argv.join(" ")
    }

    /// The program name, if any.
    pub fn program(&self) -> Option<&str> {
        self.argv.first().map(String::as_str)
    }
}

/// A command line lowered to simple commands.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ShellParse {
    /// The simple commands joined by `&&`, `||`, `;`, `|` or `&`, in order.
    pub commands: Vec<SimpleCommand>,
    /// Why the line cannot be reasoned about (command substitution, `eval`,
    /// heredoc, `sudo`, ...). Rules that allow never apply to such a line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opaque: Option<String>,
}

impl ShellParse {
    /// A parse that gave up.
    pub fn opaque(reason: impl Into<String>) -> Self {
        Self {
            commands: Vec::new(),
            opaque: Some(reason.into()),
        }
    }
}

/// Lowers a command line to a [`ShellParse`].
pub trait ShellParser: Send + Sync {
    /// Parses `command`. Never fails: what cannot be understood is reported
    /// through [`ShellParse::opaque`].
    fn parse(&self, command: &str) -> ShellParse;
}

/// Programs whose arguments are themselves a command line, or that change who
/// runs it. The policy cannot see through them.
const OPAQUE_PROGRAMS: &[&str] = &[
    "eval", "exec", "source", ".", "sudo", "su", "doas", "sh", "bash", "zsh", "dash", "fish",
    "ksh", "csh",
];

/// The built-in parser: `cosmos_llm_coding_tools::parse_shell` lowered to
/// [`ShellParse`].
///
/// Conservative by construction. Command and process substitution, here-docs,
/// computed command names or redirect targets, unparsed constructs (`if`,
/// `for`, functions), redirected groups, and programs like `eval` or `sudo`
/// all make the line opaque.
#[derive(Debug, Clone, Copy, Default)]
pub struct IrShellParser;

/// Kept for callers that used the earlier name.
pub type BasicShellParser = IrShellParser;

impl ShellParser for IrShellParser {
    fn parse(&self, command: &str) -> ShellParse {
        let ir = match parse_shell(command) {
            Ok(ir) => ir,
            Err(e) => return ShellParse::opaque(format!("unparseable: {e}")),
        };
        lower_ir(&ir)
    }
}

/// Lowers a parsed tree to the policy's view of it.
pub fn lower_ir(ir: &ShellCommandIr) -> ShellParse {
    if ir.has_opaque() {
        return ShellParse::opaque("contains shell syntax the policy does not model");
    }
    if ir.has_substitution() {
        return ShellParse::opaque("command or process substitution");
    }
    let mut bad: Option<String> = None;
    ir.walk(&mut |node| match node {
        ShellCommandIr::Subshell { redirects, .. } | ShellCommandIr::Group { redirects, .. }
            if !redirects.is_empty() =>
        {
            bad = Some("redirection applied to a group".into());
        }
        _ => {}
    });
    if let Some(reason) = bad {
        return ShellParse::opaque(reason);
    }

    let mut commands = Vec::new();
    for sc in ir.simple_commands() {
        if sc.assignments.iter().any(|a| !a.value.is_literal()) {
            return ShellParse::opaque("variable assignment is computed");
        }
        if sc.argv.is_empty() {
            if sc.redirects.is_empty() {
                continue;
            }
            return ShellParse::opaque("redirect without a command");
        }
        if !sc.argv[0].is_literal() {
            return ShellParse::opaque("command name is computed");
        }
        let mut redirects = Vec::new();
        for r in &sc.redirects {
            let op = match r.op {
                RedirectOp::Read | RedirectOp::DupIn => "<",
                RedirectOp::Write | RedirectOp::ReadWrite | RedirectOp::DupOut => ">",
                RedirectOp::Append => ">>",
                RedirectOp::Clobber => ">|",
                RedirectOp::WriteBoth => "&>",
                RedirectOp::AppendBoth => "&>>",
                RedirectOp::HereDoc | RedirectOp::HereString => {
                    return ShellParse::opaque("heredoc or here-string")
                }
                _ => return ShellParse::opaque("unknown redirection"),
            };
            let dup = matches!(r.op, RedirectOp::DupIn | RedirectOp::DupOut);
            let t = &r.target.text;
            if dup && (t == "-" || t.chars().all(|c| c.is_ascii_digit())) {
                continue; // descriptor duplication, no file involved
            }
            if !r.target.is_literal() {
                return ShellParse::opaque("redirect target is computed");
            }
            redirects.push(Redirect {
                op: op.into(),
                target: t.clone(),
            });
        }
        let argv: Vec<String> = sc.argv.iter().map(|w| w.text.clone()).collect();
        commands.push(SimpleCommand { argv, redirects });
    }
    for c in &commands {
        if let Some(p) = c.program() {
            if OPAQUE_PROGRAMS.contains(&p) {
                return ShellParse {
                    commands: commands.clone(),
                    opaque: Some(format!("`{p}` runs code the policy cannot see")),
                };
            }
        }
    }
    ShellParse {
        commands,
        opaque: None,
    }
}

/// Matches `text` against a wildcard: `*` is any run of characters, `?` one.
pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || (p[pi] != '*' && p[pi] == t[ti])) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

const FIND_UNSAFE: &[&str] = &[
    "-exec", "-execdir", "-ok", "-okdir", "-delete", "-fprint", "-fprint0", "-fprintf", "-fls",
];

const GIT_BRANCH_LISTING: &[&str] = &[
    "-a",
    "-r",
    "-v",
    "-vv",
    "--all",
    "--remotes",
    "--list",
    "-l",
    "--show-current",
    "--verbose",
];

/// Whether `cmd` is on the built-in known-safe list: it reads and cannot run
/// other code. A command with an output redirection is never safe.
pub fn is_known_safe(cmd: &SimpleCommand) -> bool {
    if cmd
        .redirects
        .iter()
        .any(|r| r.writes() && r.target != "/dev/null")
    {
        return false;
    }
    let Some(program) = cmd.program() else {
        return false;
    };
    let args = &cmd.argv[1..];
    match program {
        "ls" | "cat" | "head" | "tail" | "wc" | "pwd" | "which" | "grep" => true,
        "rg" => !args
            .iter()
            .any(|a| a == "--pre" || a.starts_with("--pre=") || a.starts_with("--hostname-bin")),
        "find" => !args.iter().any(|a| FIND_UNSAFE.contains(&a.as_str())),
        "git" => {
            let Some(sub) = args.first().map(String::as_str) else {
                return false;
            };
            let rest = &args[1..];
            let flags_ok = !rest.iter().any(|a| {
                a.starts_with("--output")
                    || a == "--ext-diff"
                    || a == "--textconv"
                    || a == "-c"
                    || a.starts_with("--exec-path")
                    || a.starts_with("--open-files-in-pager")
            });
            match sub {
                "status" | "diff" | "log" | "show" => flags_ok,
                "branch" => rest
                    .iter()
                    .all(|a| GIT_BRANCH_LISTING.contains(&a.as_str())),
                _ => false,
            }
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> ShellParse {
        IrShellParser.parse(s)
    }

    fn argvs(p: &ShellParse) -> Vec<Vec<&str>> {
        p.commands
            .iter()
            .map(|c| c.argv.iter().map(String::as_str).collect())
            .collect()
    }

    #[test]
    fn splits_on_operators() {
        let p = parse("cargo test && rm -rf ~ ; ls | wc -l || true");
        assert!(p.opaque.is_none(), "{p:?}");
        assert_eq!(
            argvs(&p),
            vec![
                vec!["cargo", "test"],
                vec!["rm", "-rf", "~"],
                vec!["ls"],
                vec!["wc", "-l"],
                vec!["true"]
            ]
        );
    }

    #[test]
    fn quotes_hide_operators() {
        let p = parse(r#"grep "a && b" 'x;y' file"#);
        assert_eq!(argvs(&p), vec![vec!["grep", "a && b", "x;y", "file"]]);
    }

    #[test]
    fn unmodelled_syntax_is_opaque() {
        for s in [
            "ls $(rm -rf x)",
            "ls `rm x`",
            "echo \"$(date)\"",
            "diff <(ls) b",
            "cat <<EOF\nhi\nEOF",
            "(cd x && ls) > out",
            "ls 'unterminated",
            "$CMD arg",
            "if true; then ls; fi",
            "sudo ls",
            "eval ls",
            "bash -c 'ls'",
            "source x.sh",
        ] {
            assert!(parse(s).opaque.is_some(), "{s}");
        }
    }

    #[test]
    fn redirects_are_lowered() {
        let p = parse("cargo test 2>&1 > out.txt 2>/dev/null");
        assert!(p.opaque.is_none(), "{p:?}");
        let c = &p.commands[0];
        assert_eq!(c.argv, vec!["cargo", "test"]);
        let targets: Vec<_> = c.redirects.iter().map(|r| r.target.as_str()).collect();
        assert_eq!(targets, vec!["out.txt", "/dev/null"]);
    }

    #[test]
    fn leading_assignments_are_dropped() {
        let p = parse("RUST_LOG=debug cargo test");
        assert_eq!(argvs(&p), vec![vec!["cargo", "test"]]);
        assert!(parse("X=$(date) ls").opaque.is_some());
    }

    #[test]
    fn wildcards() {
        assert!(wildcard_match("cargo test*", "cargo test --lib"));
        assert!(wildcard_match("cargo test*", "cargo test"));
        assert!(!wildcard_match("cargo test*", "cargo build"));
        assert!(wildcard_match("git ?ush*", "git push origin"));
        assert!(wildcard_match("*", ""));
        assert!(!wildcard_match("a", "ab"));
    }

    #[test]
    fn safe_list() {
        let safe = |s: &str| is_known_safe(&parse(s).commands[0]);
        assert!(safe("ls -la"));
        assert!(safe("git status"));
        assert!(safe("git log --oneline"));
        assert!(safe("git branch -a"));
        assert!(!safe("git branch -D x"));
        assert!(!safe("git push"));
        assert!(!safe("git diff --output=x"));
        assert!(safe("find . -name '*.rs'"));
        assert!(!safe("find . -delete"));
        assert!(!safe("find . -exec rm {} +"));
        assert!(!safe("rg --pre ./x foo"));
        assert!(!safe("cat a > b"));
        assert!(safe("cat a 2>/dev/null"));
        assert!(!safe("rm x"));
    }
}
