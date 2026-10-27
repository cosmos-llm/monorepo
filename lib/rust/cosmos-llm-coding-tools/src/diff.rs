//! Diff IR types and the function that builds them.

use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};

/// How a file changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ChangeKind {
    /// The file did not exist before.
    Created,
    /// The file existed before and after.
    Modified,
    /// The file no longer exists.
    Deleted,
}

/// One hunk of a unified diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Hunk {
    /// First line of the hunk in the old file (1-based).
    pub old_start: u32,
    /// Lines the hunk covers in the old file.
    pub old_lines: u32,
    /// First line of the hunk in the new file (1-based).
    pub new_start: u32,
    /// Lines the hunk covers in the new file.
    pub new_lines: u32,
    /// The hunk body: lines prefixed with ` `, `+` or `-`, each ending in a
    /// newline.
    pub text: String,
}

impl Hunk {
    /// The `@@ -a,b +c,d @@` line.
    pub fn header(&self) -> String {
        format!(
            "@@ -{},{} +{},{} @@",
            self.old_start, self.old_lines, self.new_start, self.new_lines
        )
    }
}

/// What changed in one file: the structure a UI renders, so it does not have
/// to parse diff text.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_coding_tools::diff::file_diff;
///
/// let d = file_diff("a.txt", Some("one\ntwo\n"), Some("one\n2\n"));
/// assert_eq!((d.added, d.removed), (1, 1));
/// assert!(d.unified().contains("-two"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct FileDiff {
    /// Path as shown to the user (root-relative where possible).
    pub path: String,
    /// Created, modified or deleted.
    pub kind: ChangeKind,
    /// Lines added.
    pub added: u32,
    /// Lines removed.
    pub removed: u32,
    /// `true` when either side is not text; `hunks` is then empty.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub binary: bool,
    /// Hunks with 3 lines of context.
    #[serde(default)]
    pub hunks: Vec<Hunk>,
}

impl FileDiff {
    /// A diff for a file that is not text.
    pub fn binary(path: impl Into<String>, kind: ChangeKind) -> Self {
        Self {
            path: path.into(),
            kind,
            added: 0,
            removed: 0,
            binary: true,
            hunks: Vec::new(),
        }
    }

    /// Renders the diff in unified format.
    pub fn unified(&self) -> String {
        if self.binary {
            return format!("Binary file {} changed\n", self.path);
        }
        let old = if self.kind == ChangeKind::Created {
            "/dev/null".to_string()
        } else {
            format!("a/{}", self.path)
        };
        let new = if self.kind == ChangeKind::Deleted {
            "/dev/null".to_string()
        } else {
            format!("b/{}", self.path)
        };
        let mut out = format!("--- {old}\n+++ {new}\n");
        for h in &self.hunks {
            out.push_str(&h.header());
            out.push('\n');
            out.push_str(&h.text);
        }
        out
    }
}

fn hunk_start(start0: usize, len: usize) -> u32 {
    (if len == 0 { start0 } else { start0 + 1 }) as u32
}

/// Diffs two versions of a file's text. `None` means the file does not exist
/// on that side.
pub fn file_diff(path: &str, before: Option<&str>, after: Option<&str>) -> FileDiff {
    let kind = match (before, after) {
        (None, _) => ChangeKind::Created,
        (_, None) => ChangeKind::Deleted,
        _ => ChangeKind::Modified,
    };
    let diff = TextDiff::from_lines(before.unwrap_or(""), after.unwrap_or(""));
    let (mut added, mut removed) = (0u32, 0u32);
    let mut hunks = Vec::new();
    for group in diff.grouped_ops(3) {
        let (Some(first), Some(last)) = (group.first(), group.last()) else {
            continue;
        };
        let old_len = last.old_range().end - first.old_range().start;
        let new_len = last.new_range().end - first.new_range().start;
        let mut text = String::new();
        for op in &group {
            for change in diff.iter_changes(op) {
                let sign = match change.tag() {
                    ChangeTag::Delete => {
                        removed += 1;
                        '-'
                    }
                    ChangeTag::Insert => {
                        added += 1;
                        '+'
                    }
                    ChangeTag::Equal => ' ',
                };
                text.push(sign);
                text.push_str(change.value());
                if change.missing_newline() {
                    text.push_str("\n\\ No newline at end of file\n");
                }
            }
        }
        hunks.push(Hunk {
            old_start: hunk_start(first.old_range().start, old_len),
            old_lines: old_len as u32,
            new_start: hunk_start(first.new_range().start, new_len),
            new_lines: new_len as u32,
            text,
        });
    }
    FileDiff {
        path: path.to_string(),
        kind,
        added,
        removed,
        binary: false,
        hunks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modified_counts_and_context() {
        let before = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n";
        let after = "1\n2\n3\n4\nFIVE\n6\n7\n8\n9\n10\n";
        let d = file_diff("f", Some(before), Some(after));
        assert_eq!((d.added, d.removed), (1, 1));
        assert_eq!(d.hunks.len(), 1);
        let h = &d.hunks[0];
        assert_eq!(
            (h.old_start, h.old_lines, h.new_start, h.new_lines),
            (2, 7, 2, 7)
        );
        assert!(h.text.contains("-5\n+FIVE\n"));
    }

    #[test]
    fn created_and_deleted() {
        let c = file_diff("n", None, Some("a\nb\n"));
        assert_eq!(c.kind, ChangeKind::Created);
        assert_eq!((c.added, c.removed), (2, 0));
        assert!(c
            .unified()
            .starts_with("--- /dev/null\n+++ b/n\n@@ -0,0 +1,2 @@\n"));
        let d = file_diff("n", Some("a\n"), None);
        assert_eq!(d.kind, ChangeKind::Deleted);
        assert_eq!(d.removed, 1);
    }

    #[test]
    fn identical_has_no_hunks() {
        let d = file_diff("n", Some("a\n"), Some("a\n"));
        assert!(d.hunks.is_empty());
    }
}
