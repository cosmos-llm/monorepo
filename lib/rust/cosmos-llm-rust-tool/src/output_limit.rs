//! Limits on what a tool result may add to the model's context (doc 180).
//!
//! One `cat` of a lockfile can add 100k tokens to the history, and every later
//! turn pays for it again. [`OutputLimit`] caps what goes into the context;
//! the full text can be spilled to a file the model pages through later.
//!
//! The pieces are independent and data-in, data-out:
//!
//! * [`OutputLimit`] / [`Truncate`] - the policy, a serde IR.
//! * [`limit_output`] - text + policy -> [`LimitedOutput`], another IR.
//! * [`SpillStore`] - where the full text of a truncated result goes.
//! * [`strip_ansi`], [`collapse_carriage_returns`], [`dedupe_lines`],
//!   [`truncate_long_lines`], [`clean_terminal_output`] - tool-specific
//!   trimming to run *before* the generic limit.
//!
//! Applying the limit to every result is the job of whoever stringifies tool
//! results (the agent loop); [`Executor::limit_result`](crate::Executor::limit_result)
//! does that step for one result.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Bytes kept back from the budget for the truncation note.
const NOTE_RESERVE: usize = 200;

/// How to choose which part of an over-long output survives.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Truncate {
    /// Keep the start.
    Head,
    /// Keep the start and the end; errors and summaries are usually at the
    /// end. `head_ratio` is the share of the budget given to the start.
    HeadTail {
        /// Fraction in `0.0..=1.0`; out-of-range values are clamped.
        head_ratio: f32,
    },
}

/// A cap on one tool result, in bytes and lines. The IR for the policy.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_tool::output_limit::{OutputLimit, Truncate};
///
/// let l = OutputLimit::new().max_bytes(10_000).strategy(Truncate::HeadTail { head_ratio: 0.3 });
/// assert_eq!(l.max_bytes, 10_000);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(default)]
pub struct OutputLimit {
    /// Byte cap, including the truncation note. Default 30,000 (~7.5k tokens).
    pub max_bytes: usize,
    /// Line cap, including the note line. Default 2,000.
    pub max_lines: usize,
    /// Which part survives.
    pub strategy: Truncate,
}

impl Default for OutputLimit {
    fn default() -> Self {
        Self {
            max_bytes: 30_000,
            max_lines: 2_000,
            strategy: Truncate::Head,
        }
    }
}

impl OutputLimit {
    /// The default limit.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the byte cap.
    pub fn max_bytes(mut self, n: usize) -> Self {
        self.max_bytes = n;
        self
    }

    /// Sets the line cap.
    pub fn max_lines(mut self, n: usize) -> Self {
        self.max_lines = n;
        self
    }

    /// Sets the truncation strategy.
    pub fn strategy(mut self, s: Truncate) -> Self {
        self.strategy = s;
        self
    }
}

/// The result of applying an [`OutputLimit`]. The IR handed to the context.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct LimitedOutput {
    /// What goes into the model's context, note included.
    pub text: String,
    /// Whether anything was cut.
    pub truncated: bool,
    /// Size of the original output, in bytes.
    pub original_bytes: usize,
    /// Lines cut from the original (0 when not truncated).
    pub omitted_lines: usize,
    /// Where the full output was written, when it was.
    pub spill_path: Option<PathBuf>,
}

/// A directory that holds the full text of truncated results.
///
/// Point it at the session store, not the workspace, so spill files never show
/// up in `git status`. The `read` tool must be allowed to read it regardless of
/// workspace roots.
#[derive(Debug, Clone, PartialEq)]
pub struct SpillStore {
    dir: PathBuf,
}

impl SpillStore {
    /// A store rooted at `dir`. The directory is created on first write.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The directory spill files are written to.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Writes `text` and returns its path. Same content, same file.
    pub fn write(&self, text: &str) -> io::Result<PathBuf> {
        fs::create_dir_all(&self.dir)?;
        let mut h = DefaultHasher::new();
        text.hash(&mut h);
        let path = self.dir.join(format!("{:016x}.txt", h.finish()));
        fs::write(&path, text)?;
        Ok(path)
    }

    /// Deletes the directory and everything in it. Call when the session is
    /// deleted. A missing directory is not an error.
    pub fn remove_all(&self) -> io::Result<()> {
        match fs::remove_dir_all(&self.dir) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

fn floor_boundary(s: &str, mut i: usize) -> usize {
    i = i.min(s.len());
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_boundary(s: &str, mut i: usize) -> usize {
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i.min(s.len())
}

/// Longest prefix of whole lines within the caps; if not even one line fits,
/// a prefix of the first line cut on a character boundary.
fn take_head(text: &str, max_bytes: usize, max_lines: usize) -> &str {
    let mut end = 0;
    for (n, line) in text.split_inclusive('\n').enumerate() {
        if n >= max_lines || end + line.len() > max_bytes {
            break;
        }
        end += line.len();
    }
    if end == 0 && max_lines > 0 {
        end = floor_boundary(text, max_bytes);
    }
    &text[..end]
}

/// Mirror of [`take_head`] for the end of the text.
fn take_tail(text: &str, max_bytes: usize, max_lines: usize) -> &str {
    let mut start = text.len();
    // The last line may lack a trailing newline; split_inclusive handles it.
    for (n, line) in text.split_inclusive('\n').rev().enumerate() {
        if n >= max_lines || text.len() - start + line.len() > max_bytes {
            break;
        }
        start -= line.len();
    }
    if start == text.len() && max_lines > 0 {
        start = ceil_boundary(text, text.len().saturating_sub(max_bytes));
    }
    &text[start..]
}

fn commas(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn note(omitted_lines: usize, omitted_bytes: usize, spill: Option<&Path>) -> String {
    match spill {
        Some(p) => format!(
            "[... {} lines omitted. Full output: {} - use read with offset/limit ...]",
            commas(omitted_lines),
            p.display()
        ),
        None => format!(
            "[... {} lines omitted ({} bytes) ...]",
            commas(omitted_lines),
            commas(omitted_bytes)
        ),
    }
}

/// Applies `limit` to `text`.
///
/// Output within both caps comes back unchanged. Otherwise the cut is made on
/// line boundaries (and on a UTF-8 boundary when a single line is itself too
/// long), a note says what was omitted, and if `spill` is given the full text
/// is written there and the note names the file. A failed spill write degrades
/// to a note without a path; it never fails the tool call.
///
/// The returned text is at most `max_bytes` long as long as `max_bytes`
/// exceeds the note reserve (about 200 bytes plus the spill path).
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_tool::output_limit::{limit_output, OutputLimit, Truncate};
///
/// let big = "line\n".repeat(10_000);
/// let out = limit_output(&big, &OutputLimit::new().max_bytes(1_000), None);
/// assert!(out.truncated);
/// assert!(out.text.len() <= 1_000);
/// assert!(out.text.contains("lines omitted"));
/// ```
pub fn limit_output(text: &str, limit: &OutputLimit, spill: Option<&SpillStore>) -> LimitedOutput {
    let total_lines = text.lines().count();
    if text.len() <= limit.max_bytes && total_lines <= limit.max_lines {
        return LimitedOutput {
            text: text.to_string(),
            truncated: false,
            original_bytes: text.len(),
            omitted_lines: 0,
            spill_path: None,
        };
    }

    let spill_path = spill.and_then(|s| s.write(text).ok());
    let reserve = NOTE_RESERVE + spill_path.as_ref().map_or(0, |p| p.as_os_str().len());
    let body_bytes = limit.max_bytes.saturating_sub(reserve);
    // One line is the note itself.
    let body_lines = limit.max_lines.saturating_sub(1);

    let (head, tail) = match limit.strategy {
        Truncate::Head => (take_head(text, body_bytes, body_lines), ""),
        Truncate::HeadTail { head_ratio } => {
            let r = if head_ratio.is_nan() {
                0.5
            } else {
                head_ratio.clamp(0.0, 1.0)
            } as f64;
            let hb = (body_bytes as f64 * r) as usize;
            let hl = (body_lines as f64 * r) as usize;
            let head = take_head(text, hb, hl);
            let rest = &text[head.len()..];
            let tail = take_tail(
                rest,
                body_bytes.saturating_sub(head.len()),
                body_lines.saturating_sub(head.lines().count()),
            );
            (head, tail)
        }
    };

    let middle = &text[head.len()..text.len() - tail.len()];
    let omitted_lines = middle.lines().count().max(usize::from(!middle.is_empty()));
    let n = note(omitted_lines, middle.len(), spill_path.as_deref());

    let mut out = String::with_capacity(head.len() + tail.len() + n.len() + 2);
    out.push_str(head);
    if !head.is_empty() && !head.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&n);
    out.push('\n');
    out.push_str(tail);

    LimitedOutput {
        text: out,
        truncated: true,
        original_bytes: text.len(),
        omitted_lines,
        spill_path,
    }
}

/// Removes ANSI escape sequences (CSI, OSC and two-byte escapes).
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_tool::output_limit::strip_ansi;
/// assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
/// ```
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match it.next() {
            // CSI: parameters, then a final byte in 0x40..=0x7e.
            Some('[') => {
                for c in it.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        break;
                    }
                }
            }
            // OSC: until BEL or ST (ESC \).
            Some(']') => {
                while let Some(c) = it.next() {
                    if c == '\x07' {
                        break;
                    }
                    if c == '\x1b' {
                        it.next();
                        break;
                    }
                }
            }
            // Two-byte escape; already consumed.
            _ => {}
        }
    }
    out
}

/// Collapses `\r` progress-bar redraws to the final state of each line, and
/// normalizes `\r\n` to `\n`.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_tool::output_limit::collapse_carriage_returns;
/// assert_eq!(collapse_carriage_returns("10%\r20%\r100%\n"), "100%\n");
/// ```
pub fn collapse_carriage_returns(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for line in s.split_inclusive('\n') {
        let (body, nl) = match line.strip_suffix('\n') {
            Some(b) => (b, "\n"),
            None => (line, ""),
        };
        let body = body.trim_end_matches('\r');
        let last = body.rsplit('\r').next().unwrap_or(body);
        out.push_str(last);
        out.push_str(nl);
    }
    out
}

/// Replaces runs of identical lines (more than `min_run`) with the first line
/// and a `[previous line repeated N times]` marker.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_tool::output_limit::dedupe_lines;
/// let s = "x\n".repeat(5);
/// assert_eq!(dedupe_lines(&s, 2), "x\n[previous line repeated 4 times]\n");
/// ```
pub fn dedupe_lines(s: &str, min_run: usize) -> String {
    let mut out = String::with_capacity(s.len());
    let lines: Vec<&str> = s.split_inclusive('\n').collect();
    let mut i = 0;
    while i < lines.len() {
        let mut j = i + 1;
        while j < lines.len() && lines[j] == lines[i] {
            j += 1;
        }
        let run = j - i;
        if run > min_run {
            out.push_str(lines[i]);
            if !lines[i].ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&format!("[previous line repeated {} times]\n", run - 1));
        } else {
            for l in &lines[i..j] {
                out.push_str(l);
            }
        }
        i = j;
    }
    out
}

/// Cuts each line longer than `max_chars` characters (minified files), marking
/// the cut.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_tool::output_limit::truncate_long_lines;
/// assert_eq!(truncate_long_lines("abcdef\nok\n", 3), "abc [... 3 more chars]\nok\n");
/// ```
pub fn truncate_long_lines(s: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(s.len().min(1 << 20));
    for line in s.split_inclusive('\n') {
        let (body, nl) = match line.strip_suffix('\n') {
            Some(b) => (b, "\n"),
            None => (line, ""),
        };
        let count = body.chars().count();
        if count > max_chars {
            let cut = body
                .char_indices()
                .nth(max_chars)
                .map_or(body.len(), |(i, _)| i);
            out.push_str(&body[..cut]);
            out.push_str(&format!(" [... {} more chars]", count - max_chars));
        } else {
            out.push_str(body);
        }
        out.push_str(nl);
    }
    out
}

/// The shell-output pipeline: strip ANSI, collapse `\r` redraws, dedupe runs
/// of identical lines (runs longer than 3).
pub fn clean_terminal_output(s: &str) -> String {
    dedupe_lines(&collapse_carriage_returns(&strip_ansi(s)), 3)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cosmos-ol-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn small_output_untouched() {
        let o = limit_output("hi\n", &OutputLimit::new(), None);
        assert!(!o.truncated);
        assert_eq!(o.text, "hi\n");
    }

    #[test]
    fn ten_megabytes_is_capped_and_spilled() {
        let big = "0123456789abcdef\n".repeat(10 * 1024 * 1024 / 17);
        let dir = tmp("big");
        let store = SpillStore::new(&dir);
        let o = limit_output(&big, &OutputLimit::new(), Some(&store));
        assert!(o.truncated);
        assert!(o.text.len() <= 30_000, "{}", o.text.len());
        assert!(o.text.lines().count() <= 2_000);
        let note = o.text.lines().last().unwrap();
        assert!(
            note.starts_with("[... ") && note.contains("Full output:"),
            "{note}"
        );
        let path = o.spill_path.unwrap();
        assert_eq!(fs::read(&path).unwrap(), big.as_bytes());
        store.remove_all().unwrap();
        assert!(!dir.exists());
    }

    #[test]
    fn multibyte_cut_is_valid_utf8() {
        // One huge line of 3-byte characters, so the cut lands mid-character.
        let s = "€".repeat(5_000);
        for strat in [Truncate::Head, Truncate::HeadTail { head_ratio: 0.5 }] {
            let o = limit_output(
                &s,
                &OutputLimit::new().max_bytes(1_001).strategy(strat),
                None,
            );
            assert!(o.truncated);
            assert!(o.text.len() <= 1_001);
            // String is valid by construction; the slicing would have panicked.
        }
    }

    #[test]
    fn head_tail_keeps_last_line() {
        let mut s = "noise\n".repeat(5_000);
        s.push_str("error: boom\n");
        let l = OutputLimit::new()
            .max_bytes(2_000)
            .strategy(Truncate::HeadTail { head_ratio: 0.3 });
        let o = limit_output(&s, &l, None);
        assert!(o.text.ends_with("error: boom\n"));
        assert!(o.text.starts_with("noise\n"));
        assert!(o.text.len() <= 2_000);
    }

    #[test]
    fn head_drops_the_end() {
        let mut s = "noise\n".repeat(5_000);
        s.push_str("error: boom\n");
        let o = limit_output(&s, &OutputLimit::new().max_bytes(2_000), None);
        assert!(!o.text.contains("boom"));
    }

    #[test]
    fn line_cap_applies() {
        let s = "a\n".repeat(100);
        let o = limit_output(&s, &OutputLimit::new().max_lines(10), None);
        assert!(o.truncated);
        assert!(o.text.lines().count() <= 10);
        assert!(o.omitted_lines >= 91);
    }

    #[test]
    fn progress_bar_collapses() {
        assert_eq!(collapse_carriage_returns("10%\r20%\r100%\n"), "100%\n");
        assert_eq!(collapse_carriage_returns("a\r\nb\n"), "a\nb\n");
    }

    #[test]
    fn ansi_is_stripped() {
        assert_eq!(
            strip_ansi("\x1b[1;31merr\x1b[0m \x1b]0;title\x07ok"),
            "err ok"
        );
    }

    #[test]
    fn repeated_lines_dedupe() {
        let s = format!("start\n{}end\n", "tick\n".repeat(341));
        let d = dedupe_lines(&s, 3);
        assert_eq!(d, "start\ntick\n[previous line repeated 340 times]\nend\n");
        assert_eq!(dedupe_lines("a\na\nb\n", 3), "a\na\nb\n");
    }

    #[test]
    fn long_lines_are_cut() {
        let s = truncate_long_lines(&"é".repeat(10), 4);
        assert_eq!(s, "éééé [... 6 more chars]");
    }

    #[test]
    fn commas_format() {
        assert_eq!(commas(14203), "14,203");
        assert_eq!(commas(999), "999");
        assert_eq!(commas(1000000), "1,000,000");
    }

    #[test]
    fn serde_shape() {
        let l = OutputLimit::new().strategy(Truncate::HeadTail { head_ratio: 0.25 });
        let v = serde_json::to_value(l).unwrap();
        assert_eq!(v["strategy"]["type"], "head_tail");
        let back: OutputLimit = serde_json::from_value(v).unwrap();
        assert_eq!(back, l);
        // Missing fields take defaults.
        let d: OutputLimit = serde_json::from_str("{}").unwrap();
        assert_eq!(d, OutputLimit::default());
    }
}
