//! A small bash parser that lowers a command line into [`ShellCommandIr`].
//!
//! It understands what a permission policy needs to see: simple commands,
//! `| && || ; &`, subshells and groups, redirections (including here-docs),
//! quoting, and every kind of substitution, which it parses recursively.
//! Compound commands (`if`, `for`, `while`, `case`, `[[`, functions) are not
//! modelled; they lower to [`ShellCommandIr::Opaque`] holding the rest of the
//! input, so nothing after them can hide.
//!
//! The parser never evaluates anything. It is conservative by construction:
//! input it cannot parse is an error or `Opaque`, never silently dropped.

use super::ir::*;

/// Why a command line did not parse.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ShellParseError {
    /// A quote, substitution or group was never closed.
    #[error("unterminated {0}")]
    Unterminated(&'static str),
    /// A token that cannot appear here.
    #[error("unexpected {0}")]
    Unexpected(String),
    /// Substitutions nested too deeply.
    #[error("commands nested too deeply")]
    TooDeep,
}

const MAX_DEPTH: usize = 32;

/// Parses a command line into a [`ShellCommandIr`].
///
/// # Errors
///
/// [`ShellParseError`] for unterminated quotes or groups and stray
/// operators. A policy should treat an error like an
/// [`Opaque`](ShellCommandIr::Opaque) node: ask.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_coding_tools::shell::{parse_shell, ShellCommandIr};
///
/// let ir = parse_shell("ls $(rm -rf x)").unwrap();
/// assert!(ir.has_substitution());
/// assert_eq!(ir.simple_commands().len(), 2);
///
/// assert!(parse_shell("echo 'unterminated").is_err());
/// assert!(matches!(
///     parse_shell("for f in *; do rm $f; done").unwrap(),
///     ShellCommandIr::Opaque { .. }
/// ));
/// ```
pub fn parse_shell(source: &str) -> Result<ShellCommandIr, ShellParseError> {
    Parser::new(source, 0).parse_list(None)
}

type Res<T> = Result<T, ShellParseError>;

struct Parser {
    s: Vec<char>,
    pos: usize,
    depth: usize,
    /// Newline position -> where to resume after the here-doc bodies that
    /// follow it.
    skips: Vec<(usize, usize)>,
}

#[derive(Default)]
struct WordBuilder {
    text: String,
    glob: bool,
    expansions: Vec<Expansion>,
    quoted: bool,
}

impl WordBuilder {
    fn finish(self) -> (Word, bool) {
        (
            Word {
                text: self.text,
                glob: self.glob,
                expansions: self.expansions,
            },
            self.quoted,
        )
    }
}

fn is_blank(c: char) -> bool {
    c == ' ' || c == '\t' || c == '\r'
}

fn is_word_end(c: char) -> bool {
    is_blank(c) || matches!(c, '\n' | ';' | '&' | '|' | '(' | ')' | '<' | '>')
}

impl Parser {
    fn new(src: &str, depth: usize) -> Self {
        Self {
            s: src.chars().collect(),
            pos: 0,
            depth,
            skips: Vec::new(),
        }
    }

    fn peek(&self) -> Option<char> {
        self.s.get(self.pos).copied()
    }

    fn at(&self, n: usize) -> Option<char> {
        self.s.get(self.pos + n).copied()
    }

    fn starts(&self, pat: &str) -> bool {
        pat.chars().enumerate().all(|(i, c)| self.at(i) == Some(c))
    }

    fn slice(&self, from: usize, to: usize) -> String {
        self.s[from..to].iter().collect()
    }

    fn skip_blanks(&mut self) {
        loop {
            match self.peek() {
                Some(c) if is_blank(c) => self.pos += 1,
                Some('\\') if self.at(1) == Some('\n') => self.pos += 2,
                Some('#')
                    if self.pos == 0
                        || self.s[self.pos - 1].is_whitespace()
                        || matches!(self.s[self.pos - 1], ';' | '&' | '|' | '(') =>
                {
                    while self.peek().is_some_and(|c| c != '\n') {
                        self.pos += 1;
                    }
                }
                _ => break,
            }
        }
    }

    /// Consumes one newline, jumping over any here-doc bodies after it.
    fn eat_newline(&mut self) {
        let at = self.pos;
        self.pos += 1;
        if let Some(&(_, resume)) = self.skips.iter().rev().find(|(nl, _)| *nl == at) {
            self.pos = resume;
        }
    }

    fn skip_newlines(&mut self) {
        loop {
            self.skip_blanks();
            if self.peek() == Some('\n') {
                self.eat_newline();
            } else {
                break;
            }
        }
    }

    fn opaque(&mut self, reason: &str) -> ShellCommandIr {
        let source = self.slice(self.pos, self.s.len());
        self.pos = self.s.len();
        ShellCommandIr::Opaque {
            reason: reason.to_string(),
            source,
        }
    }

    /// Does the closer sit at the current position as its own token?
    fn at_closer(&self, closer: char) -> bool {
        match self.peek() {
            Some(c) if c == closer => {
                closer == ')' || self.at(1).map_or(true, |n| is_word_end(n) || n == '}')
            }
            _ => false,
        }
    }

    fn parse_list(&mut self, closer: Option<char>) -> Res<ShellCommandIr> {
        if self.depth > MAX_DEPTH {
            return Err(ShellParseError::TooDeep);
        }
        let mut items = Vec::new();
        loop {
            self.skip_blanks();
            match self.peek() {
                None => {
                    if closer.is_some() {
                        return Err(ShellParseError::Unterminated("group or substitution"));
                    }
                    break;
                }
                Some('\n') => {
                    self.eat_newline();
                    continue;
                }
                Some(';') => {
                    self.pos += 1;
                    continue;
                }
                _ => {}
            }
            if let Some(c) = closer {
                if self.at_closer(c) {
                    break;
                }
            }
            if self.peek() == Some(')') {
                return Err(ShellParseError::Unexpected("`)`".into()));
            }
            let mut item = self.parse_and_or()?;
            self.skip_blanks();
            if self.peek() == Some('&') && self.at(1) != Some('&') {
                self.pos += 1;
                item = ShellCommandIr::Background {
                    command: Box::new(item),
                };
            } else {
                match self.peek() {
                    None | Some('\n') | Some(';') => {}
                    Some(c) if closer.is_some_and(|k| self.at_closer(k) && c == k) => {}
                    Some(c) => return Err(ShellParseError::Unexpected(format!("`{c}`"))),
                }
            }
            items.push(item);
        }
        Ok(match items.len() {
            1 => items.remove(0),
            _ => ShellCommandIr::Sequence { items },
        })
    }

    fn parse_and_or(&mut self) -> Res<ShellCommandIr> {
        let first = self.parse_pipeline()?;
        let mut rest = Vec::new();
        loop {
            self.skip_blanks();
            let op = if self.starts("&&") {
                ChainOp::And
            } else if self.starts("||") {
                ChainOp::Or
            } else {
                break;
            };
            self.pos += 2;
            self.skip_newlines();
            let command = self.parse_pipeline()?;
            rest.push(ChainLink { op, command });
        }
        Ok(if rest.is_empty() {
            first
        } else {
            ShellCommandIr::Chain {
                first: Box::new(first),
                rest,
            }
        })
    }

    fn parse_pipeline(&mut self) -> Res<ShellCommandIr> {
        self.skip_blanks();
        let mut negated = false;
        if self.peek() == Some('!') && self.at(1).is_some_and(is_blank) {
            negated = true;
            self.pos += 1;
        }
        let mut commands = vec![self.parse_command()?];
        loop {
            self.skip_blanks();
            if self.peek() == Some('|') && self.at(1) != Some('|') {
                self.pos += 1;
                if self.peek() == Some('&') {
                    self.pos += 1;
                }
                self.skip_newlines();
                commands.push(self.parse_command()?);
            } else {
                break;
            }
        }
        Ok(if commands.len() == 1 && !negated {
            commands.remove(0)
        } else {
            ShellCommandIr::Pipeline { commands, negated }
        })
    }

    fn bare_word(&self) -> String {
        let mut i = self.pos;
        let mut w = String::new();
        while let Some(&c) = self.s.get(i) {
            if is_word_end(c) {
                break;
            }
            w.push(c);
            i += 1;
        }
        w
    }

    fn parse_command(&mut self) -> Res<ShellCommandIr> {
        self.skip_blanks();
        match self.peek() {
            None | Some('\n') | Some(';') | Some('&') | Some('|') | Some(')') => {
                return Err(ShellParseError::Unexpected(match self.peek() {
                    Some(c) => format!("`{c}`"),
                    None => "end of input".into(),
                }));
            }
            Some('(') => {
                if self.at(1) == Some('(') {
                    return Ok(self.opaque("arithmetic command"));
                }
                self.pos += 1;
                self.depth += 1;
                let body = self.parse_list(Some(')'));
                self.depth -= 1;
                let body = body?;
                self.pos += 1;
                let redirects = self.parse_trailing_redirects()?;
                return Ok(ShellCommandIr::Subshell {
                    command: Box::new(body),
                    redirects,
                });
            }
            _ => {}
        }
        let word = self.bare_word();
        match word.as_str() {
            "{" => {
                self.pos += 1;
                self.depth += 1;
                let body = self.parse_list(Some('}'));
                self.depth -= 1;
                let body = body?;
                self.pos += 1;
                let redirects = self.parse_trailing_redirects()?;
                return Ok(ShellCommandIr::Group {
                    command: Box::new(body),
                    redirects,
                });
            }
            "if" | "for" | "while" | "until" | "case" | "select" | "function" | "[[" | "time"
            | "coproc" | "then" | "else" | "elif" | "fi" | "do" | "done" | "esac" | "}" => {
                return Ok(self.opaque(&format!("`{word}` compound command")));
            }
            _ => {}
        }
        self.parse_simple()
    }

    fn parse_trailing_redirects(&mut self) -> Res<Vec<Redirect>> {
        let mut out = Vec::new();
        loop {
            self.skip_blanks();
            match self.try_redirect()? {
                Some(r) => out.push(r),
                None => return Ok(out),
            }
        }
    }

    fn parse_simple(&mut self) -> Res<ShellCommandIr> {
        let mut cmd = SimpleCommand {
            assignments: Vec::new(),
            argv: Vec::new(),
            redirects: Vec::new(),
        };
        loop {
            self.skip_blanks();
            if let Some(r) = self.try_redirect()? {
                cmd.redirects.push(r);
                continue;
            }
            match self.peek() {
                None | Some('\n') | Some(';') | Some('&') | Some('|') | Some(')') => break,
                Some('(') => {
                    if cmd.argv.len() == 1 && cmd.assignments.is_empty() {
                        return Ok(self.opaque("function definition"));
                    }
                    return Err(ShellParseError::Unexpected("`(`".into()));
                }
                _ => {}
            }
            let start = self.pos;
            let (word, _) = self.read_word()?;
            if cmd.argv.is_empty() {
                if let Some(a) = assignment_of(&self.slice(start, self.pos), word.clone()) {
                    cmd.assignments.push(a);
                    continue;
                }
            }
            cmd.argv.push(word);
        }
        Ok(ShellCommandIr::Simple(cmd))
    }

    /// Parses a redirection at the cursor, if there is one.
    fn try_redirect(&mut self) -> Res<Option<Redirect>> {
        let save = self.pos;
        let mut fd = None;
        let mut digits = String::new();
        let mut i = self.pos;
        while let Some(&c) = self.s.get(i) {
            if c.is_ascii_digit() {
                digits.push(c);
                i += 1;
            } else {
                break;
            }
        }
        if !digits.is_empty() && matches!(self.s.get(i), Some('<') | Some('>')) {
            fd = digits.parse::<u32>().ok();
            self.pos = i;
        }
        let ops: [(&str, RedirectOp); 11] = [
            ("&>>", RedirectOp::AppendBoth),
            ("<<<", RedirectOp::HereString),
            ("<<-", RedirectOp::HereDoc),
            ("&>", RedirectOp::WriteBoth),
            ("<<", RedirectOp::HereDoc),
            (">>", RedirectOp::Append),
            (">&", RedirectOp::DupOut),
            (">|", RedirectOp::Clobber),
            ("<&", RedirectOp::DupIn),
            ("<>", RedirectOp::ReadWrite),
            (">", RedirectOp::Write),
        ];
        let mut found = None;
        for (pat, op) in ops {
            if fd.is_some() && pat.starts_with('&') {
                continue;
            }
            if self.starts(pat) {
                found = Some((pat.len(), op));
                break;
            }
        }
        if found.is_none() && self.starts("<") && self.at(1) != Some('(') {
            found = Some((1, RedirectOp::Read));
        }
        let Some((len, op)) = found else {
            self.pos = save;
            return Ok(None);
        };
        // `>(cmd)` is a process substitution, not a redirect.
        if op == RedirectOp::Write && self.at(1) == Some('(') {
            self.pos = save;
            return Ok(None);
        }
        self.pos += len;
        self.skip_blanks();
        let proc_sub = matches!(self.peek(), Some('<' | '>')) && self.at(1) == Some('(');
        if !proc_sub && self.peek().map_or(true, is_word_end) {
            return Err(ShellParseError::Unexpected(
                "redirection without a target".into(),
            ));
        }
        let (target, quoted) = self.read_word()?;
        let body = if op == RedirectOp::HereDoc {
            Some(self.read_heredoc(&target.text, quoted, len == 3)?)
        } else {
            None
        };
        Ok(Some(Redirect {
            fd,
            op,
            target,
            body,
        }))
    }

    /// Reads the here-doc body that follows the current line.
    fn read_heredoc(&mut self, delim: &str, quoted: bool, strip_tabs: bool) -> Res<Word> {
        let nl = (self.pos..self.s.len()).find(|&i| self.s[i] == '\n');
        let Some(nl) = nl else {
            return Err(ShellParseError::Unterminated("here-document"));
        };
        // A second here-doc on the same line starts after the first body.
        let mut at = nl + 1;
        if let Some(&(_, resume)) = self.skips.iter().rev().find(|(n, _)| *n == nl) {
            at = resume;
        }
        let mut body = String::new();
        let mut lines_end = None;
        let mut i = at;
        while i <= self.s.len() {
            let end = (i..self.s.len())
                .find(|&k| self.s[k] == '\n')
                .unwrap_or(self.s.len());
            let line: String = self.s[i..end].iter().collect();
            let cmp = if strip_tabs {
                line.trim_start_matches('\t')
            } else {
                line.as_str()
            };
            if cmp == delim {
                lines_end = Some((end + 1).min(self.s.len()));
                break;
            }
            body.push_str(cmp);
            body.push('\n');
            if end >= self.s.len() {
                break;
            }
            i = end + 1;
        }
        let Some(resume) = lines_end else {
            return Err(ShellParseError::Unterminated("here-document"));
        };
        self.skips.retain(|(n, _)| *n != nl);
        self.skips.push((nl, resume));
        if quoted {
            Ok(Word::literal(body))
        } else {
            self.nested_word(&body)
        }
    }

    /// Parses `text` as the inside of double quotes, to find its expansions.
    fn nested_word(&self, text: &str) -> Res<Word> {
        let mut p = Parser::new(text, self.depth + 1);
        if p.depth > MAX_DEPTH {
            return Err(ShellParseError::TooDeep);
        }
        let mut wb = WordBuilder::default();
        p.read_dquoted(&mut wb, false)?;
        Ok(wb.finish().0)
    }

    fn read_word(&mut self) -> Res<(Word, bool)> {
        let mut wb = WordBuilder::default();
        let mut first = true;
        while let Some(c) = self.peek() {
            let proc_sub = (c == '<' || c == '>') && self.at(1) == Some('(');
            if is_word_end(c) && !proc_sub {
                break;
            }
            match c {
                '\\' => {
                    self.pos += 1;
                    match self.peek() {
                        Some('\n') => self.pos += 1,
                        Some(n) => {
                            wb.text.push(n);
                            wb.quoted = true;
                            self.pos += 1;
                        }
                        None => wb.text.push('\\'),
                    }
                }
                '\'' => {
                    self.pos += 1;
                    let start = self.pos;
                    while self.peek().is_some_and(|c| c != '\'') {
                        self.pos += 1;
                    }
                    if self.peek().is_none() {
                        return Err(ShellParseError::Unterminated("single quote"));
                    }
                    wb.text.push_str(&self.slice(start, self.pos));
                    wb.quoted = true;
                    self.pos += 1;
                }
                '"' => {
                    self.pos += 1;
                    wb.quoted = true;
                    self.read_dquoted(&mut wb, true)?;
                }
                '$' => self.read_dollar(&mut wb)?,
                '`' => self.read_backtick(&mut wb)?,
                '<' | '>' => {
                    let dir = if c == '<' {
                        ProcessDirection::Input
                    } else {
                        ProcessDirection::Output
                    };
                    let start = self.pos;
                    self.pos += 2;
                    let command = self.parse_nested_list(')')?;
                    self.pos += 1;
                    wb.text.push_str(&self.slice(start, self.pos));
                    wb.expansions.push(Expansion::ProcessSubstitution {
                        direction: dir,
                        command: Box::new(command),
                    });
                }
                '~' if first => {
                    wb.expansions.push(Expansion::Tilde);
                    wb.text.push('~');
                    self.pos += 1;
                }
                '*' | '?' | '[' | '{' => {
                    wb.glob = true;
                    wb.text.push(c);
                    self.pos += 1;
                }
                _ => {
                    wb.text.push(c);
                    self.pos += 1;
                }
            }
            first = false;
        }
        Ok(wb.finish())
    }

    fn parse_nested_list(&mut self, closer: char) -> Res<ShellCommandIr> {
        let mut sub = Parser::new("", self.depth + 1);
        std::mem::swap(&mut sub.s, &mut self.s);
        sub.pos = self.pos;
        let r = sub.parse_list(Some(closer));
        std::mem::swap(&mut sub.s, &mut self.s);
        self.pos = sub.pos;
        r
    }

    /// Reads double-quoted content. With `terminated`, up to the closing
    /// quote (consumed); otherwise to the end of input, which is how here-doc
    /// bodies and `${...}` contents are scanned.
    fn read_dquoted(&mut self, wb: &mut WordBuilder, terminated: bool) -> Res<()> {
        loop {
            match self.peek() {
                None => {
                    if terminated {
                        return Err(ShellParseError::Unterminated("double quote"));
                    }
                    return Ok(());
                }
                Some('"') if terminated => {
                    self.pos += 1;
                    return Ok(());
                }
                Some('\\') => match self.at(1) {
                    Some('\n') => self.pos += 2,
                    Some(n) if matches!(n, '$' | '`' | '"' | '\\') => {
                        wb.text.push(n);
                        self.pos += 2;
                    }
                    _ => {
                        wb.text.push('\\');
                        self.pos += 1;
                    }
                },
                Some('$') => self.read_dollar(wb)?,
                Some('`') => self.read_backtick(wb)?,
                Some(c) => {
                    wb.text.push(c);
                    self.pos += 1;
                }
            }
        }
    }

    fn read_backtick(&mut self, wb: &mut WordBuilder) -> Res<()> {
        let start = self.pos;
        self.pos += 1;
        let mut inner = String::new();
        loop {
            match self.peek() {
                None => return Err(ShellParseError::Unterminated("backtick")),
                Some('`') => {
                    self.pos += 1;
                    break;
                }
                Some('\\') if matches!(self.at(1), Some('`' | '\\' | '$')) => {
                    inner.push(self.s[self.pos + 1]);
                    self.pos += 2;
                }
                Some(c) => {
                    inner.push(c);
                    self.pos += 1;
                }
            }
        }
        let command = Parser::new(&inner, self.depth + 1).parse_list(None)?;
        wb.text.push_str(&self.slice(start, self.pos));
        wb.expansions.push(Expansion::CommandSubstitution {
            command: Box::new(command),
        });
        Ok(())
    }

    fn read_dollar(&mut self, wb: &mut WordBuilder) -> Res<()> {
        let start = self.pos;
        match self.at(1) {
            Some('(') if self.at(2) == Some('(') => {
                // Arithmetic: find the matching `))`.
                let mut depth = 0usize;
                let mut i = self.pos + 2;
                let mut end = None;
                while i < self.s.len() {
                    match self.s[i] {
                        '(' => depth += 1,
                        ')' => {
                            depth -= 1;
                            if depth == 0 && self.s.get(i + 1) == Some(&')') {
                                end = Some(i);
                                break;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
                let Some(end) = end else {
                    return Err(ShellParseError::Unterminated("arithmetic expansion"));
                };
                let source = self.slice(self.pos + 3, end);
                let nested = self.nested_word(&source)?;
                wb.expansions.extend(nested.expansions);
                self.pos = end + 2;
                wb.text.push_str(&self.slice(start, self.pos));
                wb.expansions.push(Expansion::Arithmetic { source });
            }
            Some('(') => {
                self.pos += 2;
                let command = self.parse_nested_list(')')?;
                self.pos += 1;
                wb.text.push_str(&self.slice(start, self.pos));
                wb.expansions.push(Expansion::CommandSubstitution {
                    command: Box::new(command),
                });
            }
            Some('{') => {
                let mut depth = 0usize;
                let mut i = self.pos + 1;
                let mut end = None;
                while i < self.s.len() {
                    match self.s[i] {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                end = Some(i);
                                break;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
                let Some(end) = end else {
                    return Err(ShellParseError::Unterminated("${"));
                };
                let name = self.slice(self.pos + 2, end);
                let nested = self.nested_word(&name)?;
                wb.expansions.extend(nested.expansions);
                self.pos = end + 1;
                wb.text.push_str(&self.slice(start, self.pos));
                wb.expansions.push(Expansion::Variable { name });
            }
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                let mut i = self.pos + 1;
                while self
                    .s
                    .get(i)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_')
                {
                    i += 1;
                }
                let name = self.slice(self.pos + 1, i);
                self.pos = i;
                wb.text.push_str(&self.slice(start, self.pos));
                wb.expansions.push(Expansion::Variable { name });
            }
            Some(c) if c.is_ascii_digit() || "@*#?$!-".contains(c) => {
                self.pos += 2;
                wb.text.push_str(&self.slice(start, self.pos));
                wb.expansions.push(Expansion::Variable {
                    name: c.to_string(),
                });
            }
            _ => {
                wb.text.push('$');
                self.pos += 1;
            }
        }
        Ok(())
    }
}

/// Recognizes `NAME=value` and `NAME+=value` from a word's source text.
fn assignment_of(raw: &str, mut word: Word) -> Option<Assignment> {
    let eq = raw.find('=')?;
    let name = &raw[..eq];
    let bare = name.strip_suffix('+').unwrap_or(name);
    let mut chars = bare.chars();
    let first = chars.next()?;
    if !(first.is_ascii_alphabetic() || first == '_')
        || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    word.text = word.text.get(eq + 1..).unwrap_or("").to_string();
    Some(Assignment {
        name: name.to_string(),
        value: word,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argvs(src: &str) -> Vec<Vec<String>> {
        parse_shell(src)
            .unwrap()
            .simple_commands()
            .iter()
            .map(|c| c.argv.iter().map(|w| w.text.clone()).collect())
            .collect()
    }

    #[test]
    fn simple_and_quotes() {
        assert_eq!(
            argvs(r#"git commit -m "a b" 'c d' e\ f"#),
            [["git", "commit", "-m", "a b", "c d", "e f"]]
        );
    }

    #[test]
    fn operators() {
        assert_eq!(argvs("a && b || c; d | e & f").len(), 6);
        match parse_shell("a && b || c").unwrap() {
            ShellCommandIr::Chain { rest, .. } => {
                assert_eq!(rest[0].op, ChainOp::And);
                assert_eq!(rest[1].op, ChainOp::Or);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn operators_inside_quotes_are_text() {
        let ir = parse_shell(r#"echo "a && b" ';' x"#).unwrap();
        assert_eq!(ir.simple_commands().len(), 1);
    }

    #[test]
    fn substitutions_are_parsed() {
        let ir = parse_shell("echo $(rm -rf x) `id` \"$(date)\" <(ls)").unwrap();
        assert!(ir.has_substitution());
        let names: Vec<_> = ir
            .simple_commands()
            .iter()
            .map(|c| c.argv[0].text.clone())
            .collect();
        assert_eq!(names, ["echo", "rm", "id", "date", "ls"]);
    }

    #[test]
    fn substitution_hidden_in_expansions() {
        assert!(parse_shell("echo ${x:-$(rm y)}")
            .unwrap()
            .has_substitution());
        assert!(parse_shell("echo $(( $(id) + 1 ))")
            .unwrap()
            .has_substitution());
    }

    #[test]
    fn single_quoted_dollar_is_text() {
        let ir = parse_shell("echo '$(rm x)'").unwrap();
        assert!(!ir.has_substitution());
        assert_eq!(
            ir.simple_commands()[0].argv[1].as_literal(),
            Some("$(rm x)")
        );
    }

    #[test]
    fn redirects() {
        let ir = parse_shell("cmd 2>&1 > out.txt >> log &>all < in").unwrap();
        let ShellCommandIr::Simple(c) = ir else {
            panic!()
        };
        let ops: Vec<_> = c.redirects.iter().map(|r| (r.fd, r.op)).collect();
        assert_eq!(
            ops,
            [
                (Some(2), RedirectOp::DupOut),
                (None, RedirectOp::Write),
                (None, RedirectOp::Append),
                (None, RedirectOp::WriteBoth),
                (None, RedirectOp::Read),
            ]
        );
        assert_eq!(c.argv.len(), 1);
    }

    #[test]
    fn heredoc_body_and_following_commands() {
        let ir = parse_shell("cat <<EOF > f\nhello $(whoami)\nEOF\nrm x").unwrap();
        let names: Vec<_> = ir
            .simple_commands()
            .iter()
            .map(|c| c.argv[0].text.clone())
            .collect();
        assert_eq!(names, ["cat", "whoami", "rm"]);
        let quoted = parse_shell("cat <<'EOF'\n$(whoami)\nEOF\n").unwrap();
        assert!(!quoted.has_substitution());
    }

    #[test]
    fn assignments() {
        let ir = parse_shell("A=1 B+=\"x y\" make all").unwrap();
        let ShellCommandIr::Simple(c) = ir else {
            panic!()
        };
        assert_eq!(c.assignments.len(), 2);
        assert_eq!(c.assignments[1].value.text, "x y");
        assert_eq!(c.argv.len(), 2);
        // `=` later in the line is an argument.
        assert_eq!(argvs("echo A=1"), [["echo", "A=1"]]);
    }

    #[test]
    fn groups_and_subshells() {
        assert_eq!(argvs("(cd a && ls) ; { x; y; }").len(), 4);
        assert!(parse_shell("(a").is_err());
        assert!(parse_shell("a )").is_err());
    }

    #[test]
    fn compound_commands_are_opaque_and_keep_the_rest() {
        let ir = parse_shell("ls; for i in 1 2; do rm $i; done; rm -rf ~").unwrap();
        assert!(ir.has_opaque());
        let ShellCommandIr::Sequence { items } = ir else {
            panic!()
        };
        let ShellCommandIr::Opaque { source, .. } = &items[1] else {
            panic!()
        };
        assert!(source.ends_with("rm -rf ~"));
    }

    #[test]
    fn errors() {
        assert!(parse_shell("echo 'x").is_err());
        assert!(parse_shell("echo \"x").is_err());
        assert!(parse_shell("echo $(x").is_err());
        assert!(parse_shell("&& a").is_err());
        assert!(parse_shell("a >").is_err());
        assert!(parse_shell("cat <<EOF\nno end").is_err());
    }

    #[test]
    fn globs_tilde_and_variables_are_not_literal() {
        let ir = parse_shell("rm ~/x $HOME *.o").unwrap();
        let c = &ir.simple_commands()[0];
        assert!(c.argv[0].is_literal());
        assert!(!c.argv[1].is_literal());
        assert!(!c.argv[2].is_literal());
        assert!(c.argv[3].glob);
    }

    #[test]
    fn comments_and_empty() {
        assert_eq!(argvs("ls # rm -rf /\n"), [["ls"]]);
        assert_eq!(
            parse_shell("").unwrap(),
            ShellCommandIr::Sequence { items: vec![] }
        );
        assert_eq!(argvs("echo a#b"), [["echo", "a#b"]]);
    }

    #[test]
    fn negation_and_pipeline() {
        match parse_shell("! a | b |& c").unwrap() {
            ShellCommandIr::Pipeline { commands, negated } => {
                assert!(negated);
                assert_eq!(commands.len(), 3);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn function_definition_is_opaque() {
        assert!(parse_shell("f() { rm x; }").unwrap().has_opaque());
    }

    #[test]
    fn deep_nesting_is_rejected() {
        let s = format!("{}x{}", "$(".repeat(100), ")".repeat(100));
        assert_eq!(parse_shell(&s), Err(ShellParseError::TooDeep));
    }
}
