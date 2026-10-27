//! Search on disk: `glob`, `grep`, and fuzzy path matching.
//!
//! Everything walks through [`ToolFs::walk`], so `.gitignore`, `.ignore`,
//! global git excludes and the built-in skip list apply uniformly.

use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use cosmos_llm_tool::fs::{ToolFs, WalkOptions};
use cosmos_llm_tool::parameter::ParameterDef;
use cosmos_llm_tool::{ParameterType, ToolDefinition};
use globset::{GlobBuilder, GlobMatcher};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use regex::RegexBuilder;
use serde_json::{json, Value};

use crate::disk::SKIP_DIRS;
use crate::DiskFs;

/// Most paths `glob` returns.
pub const GLOB_LIMIT: usize = 100;
/// Default `head_limit` for `grep`.
pub const GREP_DEFAULT_LIMIT: usize = 250;
/// Longest line `grep` shows before cutting it.
pub const MAX_LINE_CHARS: usize = 500;

fn failed(error: impl std::fmt::Display) -> Value {
    json!({ "success": false, "error": error.to_string() })
}

/// `/`-joined normal components of `p`; drops `.` and any root.
fn clean(p: &Path) -> String {
    p.components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// A compiled glob. A pattern without `/` matches file names at any depth;
/// one with `/` matches the path relative to the search directory.
struct PathGlob {
    matcher: GlobMatcher,
    basename: bool,
}

impl PathGlob {
    fn new(pattern: &str) -> Result<Self, String> {
        let matcher = GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
            .map_err(|e| format!("invalid glob pattern: {e}"))?
            .compile_matcher();
        Ok(Self {
            matcher,
            basename: !pattern.contains('/'),
        })
    }

    fn is_match(&self, rel: &str) -> bool {
        if self.basename {
            self.matcher.is_match(rel.rsplit('/').next().unwrap_or(rel))
        } else {
            self.matcher.is_match(rel)
        }
    }
}

/// Files under `dir`, as (absolute-or-walk path, path relative to `dir`).
fn walk_rel(fs: &dyn ToolFs, dir: &Path) -> Result<Vec<(PathBuf, String)>, String> {
    let base = fs.normalize(dir);
    let mut out = Vec::new();
    for item in fs.walk(&base, &WalkOptions::default()) {
        let p = item.map_err(|e| e.to_string())?;
        let rel = p
            .strip_prefix(&base)
            .map(clean)
            .unwrap_or_else(|_| clean(&p));
        out.push((p, rel));
    }
    Ok(out)
}

/// The `glob` tool: files matching a pattern, newest first, capped at
/// [`GLOB_LIMIT`].
pub fn glob_tool(fs: Arc<dyn ToolFs>) -> ToolDefinition {
    ToolDefinition::new("glob")
        .read_only(true)
        .description(
            "Find files by glob pattern (e.g. '**/*.rs', 'src/**/*.{ts,tsx}'). Respects \
             .gitignore. Results are sorted by modification time, newest first, and \
             capped at 100.",
        )
        .param("pattern", ParameterType::String, true, "Glob pattern")
        .param(
            "path",
            ParameterType::String,
            false,
            "Directory to search (default: the workspace root)",
        )
        .handler(move |params| {
            let pattern = params["pattern"]
                .as_str()
                .ok_or("pattern must be a string")?;
            let dir = params["path"].as_str().unwrap_or("");
            let glob = match PathGlob::new(pattern) {
                Ok(g) => g,
                Err(e) => return Ok(failed(e)),
            };
            let files = match walk_rel(fs.as_ref(), Path::new(dir)) {
                Ok(f) => f,
                Err(e) => return Ok(failed(e)),
            };
            let mut hits: Vec<(Option<std::time::SystemTime>, String)> = files
                .into_iter()
                .filter(|(_, rel)| glob.is_match(rel))
                .map(|(p, _)| {
                    let mtime = fs.metadata(&p).ok().and_then(|m| m.mtime);
                    (mtime, clean(&fs.relativize(&p)))
                })
                .collect();
            hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            let total = hits.len();
            let paths: Vec<String> = hits.into_iter().take(GLOB_LIMIT).map(|h| h.1).collect();
            let mut out = json!({
                "success": true,
                "pattern": pattern,
                "count": paths.len(),
                "total": total,
                "truncated": total > paths.len(),
                "paths": paths,
            });
            if total > GLOB_LIMIT {
                out["note"] = json!(format!(
                    "(truncated, {} more; narrow the pattern)",
                    total - GLOB_LIMIT
                ));
            }
            Ok(out)
        })
}

/// Which engine `grep` uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum GrepBackend {
    /// `rg` if it is on `PATH`, else the in-process search.
    Auto,
    /// Require `rg`.
    Ripgrep,
    /// Always the in-process search.
    Builtin,
}

/// Is `rg` runnable?
pub fn rg_available() -> bool {
    Command::new("rg")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// One line of a search result.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Hit {
    /// 1-based line number.
    pub line: u64,
    /// The line, without its terminator.
    pub text: String,
    /// `false` for a context line.
    pub is_match: bool,
}

/// Hits for one file.
pub type FileHits = Vec<(String, Vec<Hit>)>;

/// What to search for.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct Query {
    /// Regex pattern.
    pub pattern: String,
    /// Ignore case.
    pub case_insensitive: bool,
    /// Only files matching this glob.
    pub glob: Option<String>,
    /// Only this file type (`rust`, `py`, ... as in `rg --type-list`).
    pub file_type: Option<String>,
    /// Lines of context around each match.
    pub context: usize,
}

impl Query {
    /// A query for `pattern` with defaults for everything else.
    pub fn new(pattern: impl Into<String>) -> Self {
        Self {
            pattern: pattern.into(),
            ..Self::default()
        }
    }

    /// Ignore case.
    pub fn case_insensitive(mut self, yes: bool) -> Self {
        self.case_insensitive = yes;
        self
    }

    /// Only files matching `glob`.
    pub fn glob(mut self, glob: impl Into<String>) -> Self {
        self.glob = Some(glob.into());
        self
    }

    /// Only files of this type.
    pub fn file_type(mut self, t: impl Into<String>) -> Self {
        self.file_type = Some(t.into());
        self
    }

    /// Lines of context around matches.
    pub fn context(mut self, n: usize) -> Self {
        self.context = n;
        self
    }
}

/// Searches with `rg --json` and parses its events.
pub fn search_rg(fs: &DiskFs, dir: &Path, q: &Query) -> Result<FileHits, String> {
    let root = fs.root();
    let base = fs.normalize(dir);
    let mut cmd = Command::new("rg");
    cmd.current_dir(root)
        .args(["--json", "--no-config", "--no-require-git", "--hidden"])
        .args(["--sort", "path"]);
    if q.case_insensitive {
        cmd.arg("-i");
    }
    if q.context > 0 {
        cmd.arg("-C").arg(q.context.to_string());
    }
    if let Some(g) = &q.glob {
        cmd.arg("-g").arg(g);
    }
    if let Some(t) = &q.file_type {
        cmd.arg("-t").arg(t);
    }
    for d in SKIP_DIRS {
        cmd.arg("-g").arg(format!("!{d}/"));
    }
    cmd.arg("-e").arg(&q.pattern).arg("--");
    match base.strip_prefix(root) {
        Ok(rel) if rel.as_os_str().is_empty() => {}
        Ok(rel) => {
            cmd.arg(rel);
        }
        Err(_) => {
            cmd.arg(&base);
        }
    }
    let out = cmd.output().map_err(|e| format!("could not run rg: {e}"))?;
    if out.status.code() == Some(2) && out.stdout.is_empty() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let mut files: FileHits = Vec::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Ok(ev) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let is_match = match ev["type"].as_str() {
            Some("match") => true,
            Some("context") => false,
            _ => continue,
        };
        let d = &ev["data"];
        let (Some(path), Some(text), Some(n)) = (
            d["path"]["text"].as_str(),
            d["lines"]["text"].as_str(),
            d["line_number"].as_u64(),
        ) else {
            continue;
        };
        let shown = clean(&fs.relativize(&root.join(path)));
        let hit = Hit {
            line: n,
            text: text.trim_end_matches(['\n', '\r']).to_string(),
            is_match,
        };
        match files.last_mut() {
            Some((p, hits)) if *p == shown => hits.push(hit),
            _ => files.push((shown, vec![hit])),
        }
    }
    Ok(files)
}

/// Searches in-process: the same walk and filters as `rg`, line by line.
pub fn search_builtin(fs: &DiskFs, dir: &Path, q: &Query) -> Result<FileHits, String> {
    let re = RegexBuilder::new(&q.pattern)
        .case_insensitive(q.case_insensitive)
        .build()
        .map_err(|e| format!("invalid regex pattern: {e}"))?;
    let glob = q.glob.as_deref().map(PathGlob::new).transpose()?;
    let types = match &q.file_type {
        Some(t) => {
            let mut b = ignore::types::TypesBuilder::new();
            b.add_defaults().select(t);
            Some(b.build().map_err(|e| e.to_string())?)
        }
        None => None,
    };
    let mut files = walk_rel(fs, dir)?;
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out: FileHits = Vec::new();
    for (p, rel) in files {
        if glob.as_ref().is_some_and(|g| !g.is_match(&rel)) {
            continue;
        }
        if types
            .as_ref()
            .is_some_and(|t| !t.matched(&p, false).is_whitelist())
        {
            continue;
        }
        let Ok(bytes) = fs.read(&p) else { continue };
        if bytes.iter().take(8192).any(|b| *b == 0) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        let lines: Vec<&str> = text.split('\n').collect();
        let lines = if lines.last() == Some(&"") {
            &lines[..lines.len() - 1]
        } else {
            &lines[..]
        };
        let matched: Vec<bool> = lines
            .iter()
            .map(|l| re.is_match(l.trim_end_matches('\r')))
            .collect();
        if !matched.iter().any(|m| *m) {
            continue;
        }
        let mut keep = vec![false; lines.len()];
        for (i, m) in matched.iter().enumerate() {
            if *m {
                let from = i.saturating_sub(q.context);
                let to = (i + q.context).min(lines.len() - 1);
                keep[from..=to].iter_mut().for_each(|k| *k = true);
            }
        }
        let hits: Vec<Hit> = (0..lines.len())
            .filter(|i| keep[*i])
            .map(|i| Hit {
                line: i as u64 + 1,
                text: lines[i].trim_end_matches('\r').to_string(),
                is_match: matched[i],
            })
            .collect();
        out.push((clean(&fs.relativize(&p)), hits));
    }
    Ok(out)
}

fn cut(line: &str) -> String {
    if line.chars().count() <= MAX_LINE_CHARS {
        return line.to_string();
    }
    let head: String = line.chars().take(MAX_LINE_CHARS).collect();
    format!("{head}... [line truncated]")
}

/// Renders results for the model. `limit` caps entries (files, count rows or
/// lines depending on `mode`).
pub fn render_hits(files: &FileHits, mode: &str, limit: usize, context: bool) -> (String, bool) {
    let mut lines: Vec<String> = Vec::new();
    match mode {
        "content" => {
            for (path, hits) in files {
                let mut prev: Option<u64> = None;
                for h in hits {
                    if context
                        && (prev.is_some_and(|p| h.line != p + 1)
                            || (prev.is_none() && !lines.is_empty()))
                    {
                        lines.push("--".into());
                    }
                    let sep = if h.is_match { ':' } else { '-' };
                    lines.push(format!("{path}{sep}{}{sep}{}", h.line, cut(&h.text)));
                    prev = Some(h.line);
                }
            }
        }
        "count" => {
            for (path, hits) in files {
                let n = hits.iter().filter(|h| h.is_match).count();
                lines.push(format!("{path}:{n}"));
            }
        }
        _ => {
            for (path, _) in files {
                lines.push(path.clone());
            }
        }
    }
    let total = lines.len();
    let truncated = total > limit;
    if truncated {
        lines.truncate(limit);
        lines.push(format!(
            "(truncated at {limit}; {} more; narrow the search)",
            total - limit
        ));
    }
    (lines.join("\n"), truncated)
}

/// The `grep` tool.
///
/// Uses `rg` when available and an in-process search otherwise; both honor
/// ignore files and return the same results. Defaults to
/// `files_with_matches`, which is cheap; the model then reads what it needs.
pub fn grep_tool(fs: Arc<DiskFs>, backend: GrepBackend) -> ToolDefinition {
    ToolDefinition::new("grep")
        .read_only(true)
        .description(
            "Search file contents with a regex. Respects .gitignore. output_mode is \
             files_with_matches (default), content (path:line:text) or count. Use glob \
             or type to narrow the files searched.",
        )
        .param(
            "pattern",
            ParameterType::String,
            true,
            "Regex to search for",
        )
        .param(
            "path",
            ParameterType::String,
            false,
            "Directory to search (default: the workspace root)",
        )
        .param(
            "glob",
            ParameterType::String,
            false,
            "Only search files matching this glob (e.g. '*.rs')",
        )
        .param(
            "type",
            ParameterType::String,
            false,
            "Only search this file type (e.g. 'rust', 'py', 'js')",
        )
        .param(
            "case_insensitive",
            ParameterType::Boolean,
            false,
            "Ignore case (default false)",
        )
        .push_param(
            ParameterDef::new(
                "output_mode",
                ParameterType::String,
                false,
                "files_with_matches (default), content, or count",
            )
            .with_enum(vec![
                json!("files_with_matches"),
                json!("content"),
                json!("count"),
            ])
            .with_default(json!("files_with_matches")),
        )
        .param(
            "context",
            ParameterType::Integer,
            false,
            "Lines of context around each match (content mode)",
        )
        .param(
            "head_limit",
            ParameterType::Integer,
            false,
            "Maximum entries to return (default 250)",
        )
        .handler(move |params| {
            let pattern = params["pattern"]
                .as_str()
                .ok_or("pattern must be a string")?;
            let mode = params["output_mode"]
                .as_str()
                .unwrap_or("files_with_matches");
            let limit = match params["head_limit"].as_i64() {
                Some(n) if n > 0 => n as usize,
                _ => GREP_DEFAULT_LIMIT,
            };
            let q = Query {
                pattern: pattern.to_string(),
                case_insensitive: params["case_insensitive"].as_bool().unwrap_or(false),
                glob: params["glob"].as_str().map(str::to_string),
                file_type: params["type"].as_str().map(str::to_string),
                context: params["context"].as_i64().unwrap_or(0).max(0) as usize,
            };
            if let Err(e) = regex::Regex::new(pattern) {
                return Ok(failed(format!("invalid regex pattern: {e}")));
            }
            let dir = Path::new(params["path"].as_str().unwrap_or(""));
            let use_rg = match backend {
                GrepBackend::Builtin => false,
                GrepBackend::Ripgrep => true,
                GrepBackend::Auto => rg_available(),
            };
            let result = if use_rg {
                search_rg(&fs, dir, &q)
            } else {
                search_builtin(&fs, dir, &q)
            };
            let files = match result {
                Ok(f) => f,
                Err(e) => return Ok(failed(e)),
            };
            let match_count: usize = files
                .iter()
                .map(|(_, h)| h.iter().filter(|h| h.is_match).count())
                .sum();
            let (output, truncated) = render_hits(&files, mode, limit, q.context > 0);
            Ok(json!({
                "success": true,
                "pattern": pattern,
                "output_mode": mode,
                "backend": if use_rg { "rg" } else { "builtin" },
                "file_count": files.len(),
                "match_count": match_count,
                "truncated": truncated,
                "output": output,
            }))
        })
}

/// A fuzzy match from [`FileIndex::query`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Match {
    /// Root-relative path.
    pub path: String,
    /// Higher is better.
    pub score: u32,
    /// Character positions in `path` that matched, for highlighting.
    pub indices: Vec<u32>,
}

/// An index of the workspace's paths for `@file` completion in a composer.
///
/// Building walks the tree once, which takes about a second on a big repo:
/// run [`build`](FileIndex::build) in a background task and swap the result
/// in when it finishes.
///
/// # Examples
///
/// ```rust
/// use std::path::Path;
/// use cosmos_llm_coding_tools::{DiskFs, search::FileIndex};
/// use cosmos_llm_tool::fs::ToolFs;
///
/// let dir = tempfile::tempdir().unwrap();
/// let fs = DiskFs::new(dir.path()).unwrap();
/// fs.write(Path::new("src/lib.rs"), b"").unwrap();
/// fs.write(Path::new("docs/readme.md"), b"").unwrap();
/// let index = FileIndex::build(&fs);
/// assert_eq!(index.query("srclib", 5)[0].path, "src/lib.rs");
/// ```
#[derive(Debug, Clone, Default)]
pub struct FileIndex {
    paths: Vec<String>,
}

impl FileIndex {
    /// Walks `fs` and indexes every file path.
    pub fn build(fs: &dyn ToolFs) -> Self {
        let mut idx = Self::default();
        idx.refresh(fs);
        idx
    }

    /// Walks again and replaces the index.
    pub fn refresh(&mut self, fs: &dyn ToolFs) {
        let mut paths: Vec<String> = fs
            .walk(Path::new(""), &WalkOptions::default())
            .filter_map(Result::ok)
            .map(|p| clean(&fs.relativize(&p)))
            .collect();
        paths.sort();
        self.paths = paths;
    }

    /// Number of indexed paths.
    pub fn len(&self) -> usize {
        self.paths.len()
    }

    /// `true` if nothing is indexed.
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// The best `limit` matches for `q`, best first. An empty query returns
    /// the first `limit` paths.
    pub fn query(&self, q: &str, limit: usize) -> Vec<Match> {
        if q.trim().is_empty() {
            return self
                .paths
                .iter()
                .take(limit)
                .map(|p| Match {
                    path: p.clone(),
                    score: 0,
                    indices: Vec::new(),
                })
                .collect();
        }
        let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
        let pattern = Pattern::parse(q, CaseMatching::Smart, Normalization::Smart);
        let mut buf = Vec::new();
        let mut out: Vec<Match> = Vec::new();
        for p in &self.paths {
            let mut indices = Vec::new();
            if let Some(score) =
                pattern.indices(Utf32Str::new(p, &mut buf), &mut matcher, &mut indices)
            {
                indices.sort_unstable();
                indices.dedup();
                out.push(Match {
                    path: p.clone(),
                    score,
                    indices,
                });
            }
        }
        out.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| a.path.len().cmp(&b.path.len()))
                .then_with(|| a.path.cmp(&b.path))
        });
        out.truncate(limit);
        out
    }
}
