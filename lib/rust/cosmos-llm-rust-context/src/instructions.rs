//! Project instruction loading: `AGENTS.md`, `CLAUDE.md` and friends (doc 190).
//!
//! [`load_instructions`] walks from the git root down to the working
//! directory, general to specific, and returns the files as an
//! [`Instructions`] IR. [`Instructions::to_sections`] lowers it into
//! [`PromptSection`]s; nothing here renders text for the model directly.
//!
//! Subdirectories below the cwd are not loaded up front. A monorepo's forty
//! `AGENTS.md` files should not all land in the prompt, so
//! [`load_dir_instructions`] loads one directory's files for a hook on the read
//! tool to inject when the agent first reads there. That hook belongs to the
//! agent core.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::plan::{CacheHint, PromptSection, SectionKind};

/// Where a loaded file came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum FileOrigin {
    /// The user-global file in the config directory.
    Global,
    /// A project file in the repo root or a directory down to the cwd.
    Project,
    /// An untracked `*.local.md` file beside a project file.
    Local,
    /// Pulled in by an `@path` import.
    Import,
}

/// One instruction file, after caps and truncation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct LoadedFile {
    /// Absolute path.
    pub path: PathBuf,
    /// Where it came from.
    pub origin: FileOrigin,
    /// The (possibly truncated) text.
    pub content: String,
    /// Whether `content` was cut at a size cap.
    #[serde(default)]
    pub truncated: bool,
    /// For imports, the file whose `@path` pulled this one in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_from: Option<PathBuf>,
}

/// The loaded instruction files, in prompt order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Instructions {
    /// Files, general to specific. An import follows the file that imports it.
    pub files: Vec<LoadedFile>,
    /// Whether any cap was hit (a file was cut, or later files were dropped).
    #[serde(default)]
    pub truncated: bool,
    /// Things worth logging: caps hit, unreadable files, refused imports.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// What to load and how much to keep.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct InstructionOptions {
    /// Project file names, in preference order. In each directory the first
    /// one found is used, so `CLAUDE.md` is a fallback for `AGENTS.md`. Add
    /// `GEMINI.md` or `.cursorrules` here.
    pub file_names: Vec<String>,
    /// Personal, untracked file names; every one found is loaded after the
    /// directory's project file.
    pub local_file_names: Vec<String>,
    /// Name of the user-global file inside `config_dir`.
    pub global_file_name: String,
    /// The user config directory (`~/.config/cosmos`). `None` resolves it from
    /// `XDG_CONFIG_HOME` / `HOME`.
    pub config_dir: Option<PathBuf>,
    /// Where the walk starts. `None` finds the git root from the cwd, falling
    /// back to the filesystem root. Imports are confined to this directory
    /// (or, when it is the filesystem root, to the cwd) and the config dir.
    pub root: Option<PathBuf>,
    /// Cap per file in bytes. Default 32 KiB.
    pub max_file_bytes: usize,
    /// Cap over all files in bytes. Default 32 KiB.
    pub max_total_bytes: usize,
    /// How deep `@path` imports may nest. Default 5.
    pub max_import_depth: usize,
}

impl Default for InstructionOptions {
    fn default() -> Self {
        Self {
            file_names: vec!["AGENTS.md".into(), "CLAUDE.md".into()],
            local_file_names: vec!["AGENTS.local.md".into(), "CLAUDE.local.md".into()],
            global_file_name: "AGENTS.md".into(),
            config_dir: None,
            root: None,
            max_file_bytes: 32 * 1024,
            max_total_bytes: 32 * 1024,
            max_import_depth: 5,
        }
    }
}

impl InstructionOptions {
    fn resolved_config_dir(&self) -> Option<PathBuf> {
        if let Some(d) = &self.config_dir {
            return Some(d.clone());
        }
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|v| !v.is_empty())
                    .map(|h| PathBuf::from(h).join(".config"))
            })?;
        Some(base.join("cosmos"))
    }
}

/// Finds the nearest ancestor of `start` (inclusive) holding a `.git` entry.
pub fn find_git_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|d| d.join(".git").exists())
        .map(Path::to_path_buf)
}

struct Loader<'a> {
    opts: &'a InstructionOptions,
    out: Instructions,
    seen: HashSet<PathBuf>,
    used: usize,
    allowed: Vec<PathBuf>,
    full: bool,
}

const TRUNC_NOTE: &str = "\n[... truncated: size cap reached]";

impl Loader<'_> {
    fn warn(&mut self, msg: String) {
        self.out.warnings.push(msg);
    }

    /// Reads and records `path`, then its imports. Skips missing or empty
    /// files and files already seen.
    fn load(&mut self, path: &Path, origin: FileOrigin, from: Option<&Path>, depth: usize) {
        if self.full {
            return;
        }
        let Ok(canon) = fs::canonicalize(path) else {
            return;
        };
        if !canon.is_file() || !self.seen.insert(canon.clone()) {
            return;
        }
        let raw = match fs::read(&canon) {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(e) => {
                self.warn(format!("{}: {e}", canon.display()));
                return;
            }
        };
        if raw.trim().is_empty() {
            return;
        }

        let remaining = self.opts.max_total_bytes.saturating_sub(self.used);
        let cap = self.opts.max_file_bytes.min(remaining);
        let (content, truncated) = if raw.len() > cap {
            let mut cut = cap;
            while !raw.is_char_boundary(cut) {
                cut -= 1;
            }
            (format!("{}{TRUNC_NOTE}", &raw[..cut]), true)
        } else {
            (raw.clone(), false)
        };
        if truncated {
            self.out.truncated = true;
            self.warn(format!("{}: truncated at {cap} bytes", canon.display()));
        }
        self.used += content.len() - if truncated { TRUNC_NOTE.len() } else { 0 };
        self.out.files.push(LoadedFile {
            path: canon.clone(),
            origin,
            content,
            truncated,
            imported_from: from.map(Path::to_path_buf),
        });
        if self.used >= self.opts.max_total_bytes {
            self.full = true;
            self.out.truncated = true;
            self.warn("total instruction size cap reached; later files dropped".into());
            return;
        }

        if depth >= self.opts.max_import_depth {
            return;
        }
        let base = canon.parent().map(Path::to_path_buf).unwrap_or_default();
        for target in import_paths(&raw) {
            let candidate = if let Some(rest) = target.strip_prefix("~/") {
                match std::env::var_os("HOME") {
                    Some(h) => PathBuf::from(h).join(rest),
                    None => continue,
                }
            } else {
                base.join(&target)
            };
            let Ok(resolved) = fs::canonicalize(&candidate) else {
                continue;
            };
            if !resolved.is_file() {
                continue;
            }
            if !self.allowed.iter().any(|a| resolved.starts_with(a)) {
                self.warn(format!(
                    "{}: import {} is outside the repo and config dir; skipped",
                    canon.display(),
                    resolved.display()
                ));
                continue;
            }
            self.load(&resolved, FileOrigin::Import, Some(&canon), depth + 1);
        }
    }

    fn load_dir(&mut self, dir: &Path) {
        for name in &self.opts.file_names {
            let p = dir.join(name);
            if p.is_file() {
                self.load(&p, FileOrigin::Project, None, 0);
                break;
            }
        }
        for name in &self.opts.local_file_names {
            self.load(&dir.join(name), FileOrigin::Local, None, 0);
        }
    }
}

/// `@path` tokens outside code fences and inline code. A token is `@`
/// preceded by whitespace or line start; trailing punctuation is trimmed.
/// Whether the target exists is the caller's business: `@types/node` or a
/// handle simply does not resolve and is ignored.
fn import_paths(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        // Drop inline code spans.
        let mut visible = String::new();
        for (i, part) in line.split('`').enumerate() {
            if i % 2 == 0 {
                visible.push_str(part);
                visible.push(' ');
            }
        }
        for word in visible.split_whitespace() {
            if let Some(p) = word.strip_prefix('@') {
                let p = p.trim_end_matches(|c: char| ".,;:!?)\"'".contains(c));
                if !p.is_empty() && !p.contains('@') && !found.iter().any(|f| f == p) {
                    found.push(p.to_string());
                }
            }
        }
    }
    found
}

fn new_loader<'a>(opts: &'a InstructionOptions, allowed: Vec<PathBuf>) -> Loader<'a> {
    Loader {
        opts,
        out: Instructions::default(),
        seen: HashSet::new(),
        used: 0,
        allowed,
        full: false,
    }
}

/// Loads the instruction files that apply at `cwd`.
///
/// Order, each appended general to specific: the user-global file, then for
/// every directory from the git root (or filesystem root outside a repo) down
/// to `cwd` the project file (first of `file_names` found) and any local
/// files. `@path` imports are loaded right after the file that names them,
/// each file at most once, nested at most `max_import_depth` deep, and only
/// from inside the repo or the config directory.
///
/// Caps: `max_file_bytes` per file and `max_total_bytes` overall. Past a cap a
/// file is cut with a note and [`Instructions::truncated`] is set; the reason
/// is recorded in [`Instructions::warnings`].
///
/// # Errors
///
/// Returns an error if `cwd` does not exist.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_context::{load_instructions, InstructionOptions};
///
/// let dir = std::env::temp_dir();
/// let mut opts = InstructionOptions::default();
/// opts.config_dir = Some(dir.join("no-such-config-dir"));
/// let ins = load_instructions(&dir, &opts).unwrap();
/// assert!(ins.files.iter().all(|f| f.content.len() <= 33 * 1024));
/// ```
pub fn load_instructions(cwd: &Path, opts: &InstructionOptions) -> io::Result<Instructions> {
    let cwd = fs::canonicalize(cwd)?;
    let git_root = opts.root.clone().or_else(|| find_git_root(&cwd));
    let root = git_root
        .as_deref()
        .map(|r| fs::canonicalize(r).unwrap_or_else(|_| r.to_path_buf()));

    let mut allowed = Vec::new();
    // Outside a repo imports may not roam the whole disk.
    allowed.push(root.clone().unwrap_or_else(|| cwd.clone()));
    let config_dir = opts.resolved_config_dir();
    if let Some(c) = &config_dir {
        allowed.push(fs::canonicalize(c).unwrap_or_else(|_| c.clone()));
    }
    let mut l = new_loader(opts, allowed);

    if let Some(c) = &config_dir {
        l.load(&c.join(&opts.global_file_name), FileOrigin::Global, None, 0);
    }

    let mut dirs: Vec<&Path> = cwd
        .ancestors()
        .take_while(|d| root.as_deref().map_or(true, |r| d.starts_with(r)))
        .collect();
    dirs.reverse();
    for d in dirs {
        l.load_dir(d);
    }
    Ok(l.out)
}

/// Loads the instruction files of a single directory (no global file, no
/// ancestors), for lazy injection when the agent first reads below the cwd.
///
/// `root` bounds imports, as in [`InstructionOptions::root`].
///
/// # Errors
///
/// Returns an error if `dir` does not exist.
pub fn load_dir_instructions(
    dir: &Path,
    root: &Path,
    opts: &InstructionOptions,
) -> io::Result<Instructions> {
    let dir = fs::canonicalize(dir)?;
    let root = fs::canonicalize(root)?;
    let mut l = new_loader(opts, vec![root]);
    l.load_dir(&dir);
    Ok(l.out)
}

impl Instructions {
    /// Lowers the files into prompt sections, one per file, so section
    /// boundaries and sources stay visible to `/context`.
    ///
    /// The last section carries the cache breakpoint: instructions are stable
    /// within a session, and the environment that follows is not.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_context::Instructions;
    ///
    /// assert!(Instructions::default().to_sections().is_empty());
    /// ```
    pub fn to_sections(&self) -> Vec<PromptSection> {
        let mut sections: Vec<PromptSection> = self
            .files
            .iter()
            .map(|f| {
                PromptSection::text(
                    SectionKind::Instructions,
                    format!(
                        "Instructions from {}:\n\n{}",
                        f.path.display(),
                        f.content.trim_end()
                    ),
                )
                .with_source(f.path.clone())
            })
            .collect();
        if let Some(last) = sections.last_mut() {
            last.cache = Some(CacheHint::Ephemeral);
        }
        sections
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(p: &Path, text: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    fn opts(cfg: &Path) -> InstructionOptions {
        InstructionOptions {
            config_dir: Some(cfg.to_path_buf()),
            ..InstructionOptions::default()
        }
    }

    fn names(ins: &Instructions) -> Vec<String> {
        ins.files
            .iter()
            .map(|f| f.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn ancestors_load_repo_first() {
        let t = tempfile::tempdir().unwrap();
        let repo = t.path().join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        w(&repo.join("AGENTS.md"), "root rules");
        w(&repo.join("pkg/CLAUDE.md"), "pkg rules");
        w(&repo.join("pkg/other/AGENTS.md"), "sibling, must not load");
        fs::create_dir_all(repo.join("pkg/sub")).unwrap();
        let cfg = t.path().join("cfg");
        w(&cfg.join("AGENTS.md"), "global");

        let ins = load_instructions(&repo.join("pkg/sub"), &opts(&cfg)).unwrap();
        let text: Vec<_> = ins.files.iter().map(|f| f.content.as_str()).collect();
        assert_eq!(text, ["global", "root rules", "pkg rules"]);
        assert_eq!(ins.files[0].origin, FileOrigin::Global);
        assert!(!ins.truncated);
    }

    #[test]
    fn agents_wins_over_claude_and_local_follows() {
        let t = tempfile::tempdir().unwrap();
        let repo = t.path().join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        w(&repo.join("AGENTS.md"), "a");
        w(&repo.join("CLAUDE.md"), "c");
        w(&repo.join("CLAUDE.local.md"), "mine");
        let ins = load_instructions(&repo, &opts(&t.path().join("cfg"))).unwrap();
        assert_eq!(names(&ins), ["AGENTS.md", "CLAUDE.local.md"]);
        assert_eq!(ins.files[1].origin, FileOrigin::Local);
    }

    #[test]
    fn import_cycle_loads_each_file_once() {
        let t = tempfile::tempdir().unwrap();
        let repo = t.path().join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        w(&repo.join("AGENTS.md"), "top\n@docs/a.md\n");
        w(&repo.join("docs/a.md"), "a\n@b.md\n");
        w(&repo.join("docs/b.md"), "b\n@a.md and @../AGENTS.md\n");
        let ins = load_instructions(&repo, &opts(&t.path().join("cfg"))).unwrap();
        assert_eq!(names(&ins), ["AGENTS.md", "a.md", "b.md"]);
        assert_eq!(ins.files[1].origin, FileOrigin::Import);
        assert_eq!(
            ins.files[2].imported_from.as_ref(),
            Some(&ins.files[1].path)
        );
    }

    #[test]
    fn imports_are_depth_limited_and_confined() {
        let t = tempfile::tempdir().unwrap();
        let repo = t.path().join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        w(&repo.join("AGENTS.md"), "@1.md");
        for i in 1..=8 {
            w(
                &repo.join(format!("{i}.md")),
                &format!("n{i}\n@{}.md", i + 1),
            );
        }
        w(&t.path().join("secret.md"), "outside");
        w(&repo.join("1.md"), "n1\n@2.md\n@../secret.md\n");
        let ins = load_instructions(&repo, &opts(&t.path().join("cfg"))).unwrap();
        // AGENTS.md at depth 0, imports at depth 1..=5.
        assert_eq!(ins.files.len(), 6);
        assert!(ins.files.iter().all(|f| f.content != "outside"));
        assert!(ins.warnings.iter().any(|w| w.contains("outside")));
    }

    #[test]
    fn code_fences_and_non_files_are_ignored() {
        let found =
            import_paths("see @a.md.\n```\n@no.md\n```\nuse `@x.md` and @types/node, a@b.c\n");
        assert_eq!(found, ["a.md", "types/node"]);
    }

    #[test]
    fn per_file_cap_truncates_with_note() {
        let t = tempfile::tempdir().unwrap();
        let repo = t.path().join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        w(&repo.join("AGENTS.md"), &"é".repeat(40_000));
        let ins = load_instructions(&repo, &opts(&t.path().join("cfg"))).unwrap();
        assert!(ins.truncated);
        let f = &ins.files[0];
        assert!(f.truncated);
        assert!(f.content.ends_with("size cap reached]"));
        assert!(f.content.len() <= 32 * 1024 + TRUNC_NOTE.len());
        assert!(ins.warnings[0].contains("truncated"));
    }

    #[test]
    fn total_cap_drops_later_files() {
        let t = tempfile::tempdir().unwrap();
        let repo = t.path().join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        w(&repo.join("AGENTS.md"), &"x".repeat(20_000));
        w(&repo.join("sub/AGENTS.md"), &"y".repeat(20_000));
        w(&repo.join("sub/deeper/AGENTS.md"), "never loaded");
        let ins =
            load_instructions(&repo.join("sub/deeper"), &opts(&t.path().join("cfg"))).unwrap();
        assert!(ins.truncated);
        assert_eq!(ins.files.len(), 2);
        assert!(ins.files[1].truncated);
    }

    #[test]
    fn non_repo_loads_global_only() {
        let t = tempfile::tempdir().unwrap();
        let cwd = t.path().join("a/b");
        fs::create_dir_all(&cwd).unwrap();
        let cfg = t.path().join("cfg");
        w(&cfg.join("AGENTS.md"), "global");
        let ins = load_instructions(&cwd, &opts(&cfg)).unwrap();
        assert_eq!(names(&ins), ["AGENTS.md"]);
        assert_eq!(ins.files[0].origin, FileOrigin::Global);
    }

    #[test]
    fn dir_loader_and_sections() {
        let t = tempfile::tempdir().unwrap();
        let repo = t.path().join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        w(&repo.join("AGENTS.md"), "root");
        w(&repo.join("pkg/AGENTS.md"), "pkg");
        let o = opts(&t.path().join("cfg"));
        let ins = load_dir_instructions(&repo.join("pkg"), &repo, &o).unwrap();
        assert_eq!(ins.files.len(), 1);

        let all = load_instructions(&repo.join("pkg"), &o).unwrap();
        let secs = all.to_sections();
        assert_eq!(secs.len(), 2);
        assert!(secs[0].cache.is_none() && secs[1].cache.is_some());
        assert!(secs[1].rendered().contains("pkg"));
        assert!(secs[1].source.is_some());
    }

    #[test]
    fn missing_cwd_is_an_error() {
        let o = InstructionOptions::default();
        assert!(load_instructions(Path::new("/definitely/not/here"), &o).is_err());
    }
}
