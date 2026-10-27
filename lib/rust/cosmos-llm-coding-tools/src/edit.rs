//! Exact-string edits as pure functions.
//!
//! Everything here works on text in memory. The tools in [`crate::tools`]
//! wrap it with file access, the read-before-write guard and tracking. A
//! preview and the real edit run the same functions, so they cannot disagree.

use serde::{Deserialize, Serialize};
use similar::TextDiff;

/// One replacement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct EditSpec {
    /// Text to find. Empty creates a file (single `edit` only).
    pub old_string: String,
    /// Replacement. Must differ from `old_string`.
    pub new_string: String,
    /// Replace every occurrence instead of requiring a unique one.
    #[serde(default)]
    pub replace_all: bool,
}

impl EditSpec {
    /// Builds a single-occurrence edit.
    pub fn new(old_string: impl Into<String>, new_string: impl Into<String>) -> Self {
        Self {
            old_string: old_string.into(),
            new_string: new_string.into(),
            replace_all: false,
        }
    }

    /// Sets [`replace_all`](Self::replace_all).
    pub fn replace_all(mut self, yes: bool) -> Self {
        self.replace_all = yes;
        self
    }
}

/// Why an edit could not be applied. The messages are written for the model.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum EditError {
    /// `old_string` equals `new_string`.
    #[error("old_string and new_string are identical; nothing to change")]
    Identical,
    /// `old_string` is empty but the file has content.
    #[error("old_string is empty but the file already has content; provide the text to replace")]
    EmptyOld,
    /// No match, even ignoring whitespace differences.
    #[error("old_string was not found in the file.{0}")]
    NotFound(String),
    /// Several matches and `replace_all` is off.
    #[error("found {0} matches of old_string; add surrounding context to make it unique, or set replace_all")]
    Ambiguous(usize),
    /// The whitespace-insensitive search found several candidates.
    #[error("old_string matches {0} places once whitespace is ignored; add surrounding context to make it unique")]
    AmbiguousFuzzy(usize),
}

/// The result of applying one edit.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Applied {
    /// The edited text (LF line endings).
    pub content: String,
    /// How many places changed.
    pub replacements: usize,
    /// `true` when the match ignored whitespace differences and
    /// `new_string` was re-indented to fit.
    pub whitespace_fallback: bool,
}

/// Does `text` use CRLF line endings?
pub fn uses_crlf(text: &str) -> bool {
    text.contains("\r\n")
}

/// Converts CRLF to LF.
pub fn to_lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// Converts LF to CRLF.
pub fn to_crlf(text: &str) -> String {
    to_lf(text).replace('\n', "\r\n")
}

/// Applies one edit to LF-normalized `content`.
///
/// 1. Count exact occurrences of `old_string`.
/// 2. With none, retry line by line ignoring runs of whitespace; accept only
///    a unique match and re-indent `new_string` to the matched block.
/// 3. With several and no `replace_all`, fail.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_coding_tools::edit::{apply_edit, EditSpec};
///
/// let r = apply_edit("a\nb\n", &EditSpec::new("b", "c")).unwrap();
/// assert_eq!(r.content, "a\nc\n");
/// assert!(apply_edit("x x", &EditSpec::new("x", "y")).is_err());
/// assert_eq!(apply_edit("x x", &EditSpec::new("x", "y").replace_all(true)).unwrap().content, "y y");
/// ```
pub fn apply_edit(content: &str, spec: &EditSpec) -> Result<Applied, EditError> {
    let old = to_lf(&spec.old_string);
    let new = to_lf(&spec.new_string);
    if old == new {
        return Err(EditError::Identical);
    }
    if old.is_empty() {
        return if content.is_empty() {
            Ok(Applied {
                content: new,
                replacements: 1,
                whitespace_fallback: false,
            })
        } else {
            Err(EditError::EmptyOld)
        };
    }
    let count = content.matches(&old).count();
    match count {
        0 => whitespace_edit(content, &old, &new),
        1 => Ok(Applied {
            content: content.replacen(&old, &new, 1),
            replacements: 1,
            whitespace_fallback: false,
        }),
        n if spec.replace_all => Ok(Applied {
            content: content.replace(&old, &new),
            replacements: n,
            whitespace_fallback: false,
        }),
        n => Err(EditError::Ambiguous(n)),
    }
}

/// Applies edits in order to one in-memory text. If any fails, returns its
/// index and error, and the caller writes nothing.
pub fn apply_edits(content: &str, specs: &[EditSpec]) -> Result<Applied, (usize, EditError)> {
    let mut cur = content.to_string();
    let mut replacements = 0;
    let mut fuzzy = false;
    for (i, s) in specs.iter().enumerate() {
        let r = apply_edit(&cur, s).map_err(|e| (i, e))?;
        cur = r.content;
        replacements += r.replacements;
        fuzzy |= r.whitespace_fallback;
    }
    Ok(Applied {
        content: cur,
        replacements,
        whitespace_fallback: fuzzy,
    })
}

fn collapse(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn indent_of(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

fn whitespace_edit(content: &str, old: &str, new: &str) -> Result<Applied, EditError> {
    let old_lines: Vec<&str> = old.strip_suffix('\n').unwrap_or(old).split('\n').collect();
    let want: Vec<String> = old_lines.iter().map(|l| collapse(l)).collect();
    if want.iter().all(String::is_empty) {
        return Err(EditError::NotFound(String::new()));
    }
    let lines: Vec<&str> = content.split('\n').collect();
    let have: Vec<String> = lines.iter().map(|l| collapse(l)).collect();
    let n = want.len();
    let mut hits: Vec<usize> = Vec::new();
    if lines.len() >= n {
        for i in 0..=(lines.len() - n) {
            if have[i..i + n] == want[..] {
                hits.push(i);
            }
        }
    }
    match hits.len() {
        0 => Err(EditError::NotFound(nearby(&lines, old_lines[0]))),
        1 => {
            let at = hits[0];
            let block_indent = indent_of(lines[at]);
            let old_indent = indent_of(old_lines[0]);
            let new_text = new.strip_suffix('\n').unwrap_or(new);
            let unit = indent_unit(&old_lines, new_text);
            let reindented: Vec<String> = new_text
                .split('\n')
                .map(|l| {
                    if l.trim().is_empty() {
                        return String::new();
                    }
                    reindent(l, old_indent, block_indent, unit)
                })
                .collect();
            let mut out: Vec<String> = lines[..at].iter().map(|s| s.to_string()).collect();
            if !new.is_empty() {
                out.extend(reindented);
            }
            out.extend(lines[at + n..].iter().map(|s| s.to_string()));
            Ok(Applied {
                content: out.join("\n"),
                replacements: 1,
                whitespace_fallback: true,
            })
        }
        k => Err(EditError::AmbiguousFuzzy(k)),
    }
}

/// Width of one indent level in space-indented text: the smallest non-zero
/// run of leading spaces among the given lines, else 4.
fn indent_unit(old_lines: &[&str], new_text: &str) -> usize {
    old_lines
        .iter()
        .copied()
        .chain(new_text.split('\n'))
        .filter(|l| !l.trim().is_empty())
        .map(|l| indent_of(l).chars().take_while(|c| *c == ' ').count())
        .filter(|n| *n > 0)
        .min()
        .unwrap_or(4)
}

fn reindent(line: &str, old_indent: &str, block_indent: &str, unit: usize) -> String {
    let ws = indent_of(line);
    let body = &line[ws.len()..];
    let rest = ws.strip_prefix(old_indent).unwrap_or(ws);
    let rest = if block_indent.contains('\t') && rest.contains(' ') && !rest.contains('\t') {
        "\t".repeat(rest.len() / unit.max(1))
    } else if !block_indent.contains('\t') && rest.contains('\t') {
        rest.replace('\t', &" ".repeat(unit))
    } else {
        rest.to_string()
    };
    format!("{block_indent}{rest}{body}")
}

/// A few lines of the file closest to the first line of `old_first`, so the
/// model can correct itself.
fn nearby(lines: &[&str], old_first: &str) -> String {
    let target = old_first.trim();
    if target.is_empty() || lines.is_empty() {
        return String::new();
    }
    let best = lines
        .iter()
        .enumerate()
        .map(|(i, l)| (i, TextDiff::from_chars(target, l.trim()).ratio()))
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    let Some((i, ratio)) = best else {
        return String::new();
    };
    if ratio < 0.4 {
        return String::new();
    }
    let from = i.saturating_sub(1);
    let to = (i + 2).min(lines.len());
    let mut s = String::from(" Closest lines in the file:\n");
    for (k, l) in lines.iter().enumerate().take(to).skip(from) {
        s.push_str(&format!("{:>5}\t{}\n", k + 1, l));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_multiple_none_and_replace_all() {
        let c = "alpha\nbeta\nalpha\n";
        assert_eq!(
            apply_edit(c, &EditSpec::new("beta", "gamma"))
                .unwrap()
                .content,
            "alpha\ngamma\nalpha\n"
        );
        assert_eq!(
            apply_edit(c, &EditSpec::new("alpha", "x")),
            Err(EditError::Ambiguous(2))
        );
        let all = apply_edit(c, &EditSpec::new("alpha", "x").replace_all(true)).unwrap();
        assert_eq!(all.content, "x\nbeta\nx\n");
        assert_eq!(all.replacements, 2);
        assert!(matches!(
            apply_edit(c, &EditSpec::new("zzz", "q")),
            Err(EditError::NotFound(_))
        ));
        assert_eq!(
            apply_edit(c, &EditSpec::new("beta", "beta")),
            Err(EditError::Identical)
        );
    }

    #[test]
    fn not_found_quotes_nearby_lines() {
        let c = "fn main() {\n    println!(\"hi\");\n}\n";
        let Err(EditError::NotFound(hint)) =
            apply_edit(c, &EditSpec::new("println!(\"hello\");", "x"))
        else {
            panic!()
        };
        assert!(hint.contains("println!(\"hi\");"), "{hint}");
    }

    #[test]
    fn empty_old_only_on_empty_content() {
        assert_eq!(
            apply_edit("", &EditSpec::new("", "new")).unwrap().content,
            "new"
        );
        assert_eq!(
            apply_edit("x", &EditSpec::new("", "new")),
            Err(EditError::EmptyOld)
        );
    }

    #[test]
    fn whitespace_fallback_reindents_with_tabs() {
        let c = "fn a() {\n\tif x {\n\t\tlet a = 1;\n\t\tlet b = 2;\n\t}\n}\n";
        let old = "    if x {\n        let a = 1;\n        let b = 2;\n    }";
        let new = "    if x {\n        let a = 1;\n        let c = 3;\n    }";
        let r = apply_edit(c, &EditSpec::new(old, new)).unwrap();
        assert!(r.whitespace_fallback);
        assert_eq!(
            r.content,
            "fn a() {\n\tif x {\n\t\tlet a = 1;\n\t\tlet c = 3;\n\t}\n}\n"
        );
    }

    #[test]
    fn whitespace_fallback_needs_a_unique_match() {
        let c = "\tfoo(1);\n\tfoo(1);\n";
        assert_eq!(
            apply_edit(c, &EditSpec::new("  foo(1);", "bar();")),
            Err(EditError::AmbiguousFuzzy(2))
        );
    }

    #[test]
    fn whitespace_fallback_collapses_inner_runs() {
        let c = "let   x  =  1;\nlet y = 2;\n";
        let r = apply_edit(c, &EditSpec::new("let x = 1;", "let x = 10;")).unwrap();
        assert_eq!(r.content, "let x = 10;\nlet y = 2;\n");
        assert!(r.whitespace_fallback);
    }

    #[test]
    fn multi_edit_is_sequential_and_reports_the_index() {
        let specs = [
            EditSpec::new("a", "b"),
            EditSpec::new("b", "c"),
            EditSpec::new("zzz", "q"),
        ];
        let (i, _) = apply_edits("a", &specs).unwrap_err();
        assert_eq!(i, 2);
        assert_eq!(apply_edits("a", &specs[..2]).unwrap().content, "c");
    }

    #[test]
    fn line_endings() {
        assert!(uses_crlf("a\r\nb"));
        assert_eq!(to_crlf("a\nb\r\n"), "a\r\nb\r\n");
        assert_eq!(to_lf("a\r\nb"), "a\nb");
    }
}
