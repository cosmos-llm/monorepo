//! Environment facts for the system prompt (doc 190).
//!
//! [`probe_environment`] gathers cwd, git state, platform and date into an
//! [`EnvironmentFacts`] IR; [`EnvironmentFacts::render`] turns it into text.
//! Probing is done once at thread start and again when the cwd changes. It
//! shells out to `git` and `uname`, so it belongs to the host, not to config
//! evaluation; anything that fails is simply omitted.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

/// Git facts about the working directory.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct GitFacts {
    /// Current branch, `None` when detached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// The default branch (`origin/HEAD`, else `main`/`master` if present).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_branch: Option<String>,
    /// `git status --short`, capped at the probe's line limit.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub status: Vec<String>,
    /// Whether `status` was cut short.
    #[serde(default)]
    pub status_truncated: bool,
    /// Subjects of the most recent commits, newest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_commits: Vec<String>,
}

/// Facts about the environment the agent works in.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct EnvironmentFacts {
    /// Working directory.
    pub cwd: PathBuf,
    /// Root of the enclosing git repository, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_root: Option<PathBuf>,
    /// Whether `cwd` is inside a git repository.
    #[serde(default)]
    pub is_repo: bool,
    /// Operating system (`linux`, `macos`, ...).
    pub os: String,
    /// CPU architecture.
    pub arch: String,
    /// OS version string, when it could be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
    /// The user's shell, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    /// Today's date, `YYYY-MM-DD`.
    pub date: String,
    /// Model name, supplied by the caller (doc 050 will supply it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The model's knowledge cutoff, supplied by the caller.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub knowledge_cutoff: Option<String>,
    /// Git state, when `is_repo`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<GitFacts>,
}

/// What to probe and how much to keep.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ProbeOptions {
    /// Model name to record.
    pub model: Option<String>,
    /// Knowledge cutoff to record.
    pub knowledge_cutoff: Option<String>,
    /// Date override (`YYYY-MM-DD`); `None` reads the clock.
    pub date: Option<String>,
    /// Whether to run `git` at all.
    pub git: bool,
    /// Cap on `git status --short` lines. Default 30.
    pub max_status_lines: usize,
    /// How many commit subjects to keep. Default 5.
    pub recent_commits: usize,
}

impl Default for ProbeOptions {
    fn default() -> Self {
        Self {
            model: None,
            knowledge_cutoff: None,
            date: None,
            git: true,
            max_status_lines: 30,
            recent_commits: 5,
        }
    }
}

fn run(dir: &Path, program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program)
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim_end().to_string();
    Some(s)
}

/// Days since 1970-01-01 to a civil date (proleptic Gregorian).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Today's date in UTC as `YYYY-MM-DD`.
pub fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    format!("{y:04}-{m:02}-{d:02}")
}

fn probe_git(cwd: &Path, opts: &ProbeOptions) -> Option<(PathBuf, GitFacts)> {
    let root = run(cwd, "git", &["rev-parse", "--show-toplevel"])?;
    let branch = run(cwd, "git", &["branch", "--show-current"]).filter(|b| !b.is_empty());
    let default_branch = run(
        cwd,
        "git",
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    )
    .map(|b| b.strip_prefix("origin/").unwrap_or(&b).to_string())
    .or_else(|| {
        ["main", "master"].into_iter().find_map(|b| {
            run(cwd, "git", &["rev-parse", "--verify", "--quiet", b]).map(|_| b.to_string())
        })
    });
    let all: Vec<String> = run(cwd, "git", &["status", "--short"])
        .map(|s| s.lines().map(str::to_string).collect())
        .unwrap_or_default();
    let status_truncated = all.len() > opts.max_status_lines;
    let status = all.into_iter().take(opts.max_status_lines).collect();
    let n = format!("-{}", opts.recent_commits);
    let recent_commits = if opts.recent_commits == 0 {
        Vec::new()
    } else {
        run(cwd, "git", &["log", &n, "--format=%s"])
            .map(|s| s.lines().map(str::to_string).collect())
            .unwrap_or_default()
    };
    Some((
        PathBuf::from(root),
        GitFacts {
            branch,
            default_branch,
            status,
            status_truncated,
            recent_commits,
        },
    ))
}

/// Gathers [`EnvironmentFacts`] for `cwd`.
///
/// Never fails: facts that cannot be read are left out.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_context::{probe_environment, ProbeOptions};
///
/// let mut opts = ProbeOptions::default();
/// opts.git = false;
/// opts.date = Some("2026-01-02".into());
/// let env = probe_environment(std::path::Path::new("."), &opts);
/// assert_eq!(env.date, "2026-01-02");
/// assert!(env.git.is_none());
/// ```
pub fn probe_environment(cwd: &Path, opts: &ProbeOptions) -> EnvironmentFacts {
    let (git_root, git) = if opts.git {
        match probe_git(cwd, opts) {
            Some((r, g)) => (Some(r), Some(g)),
            None => (None, None),
        }
    } else {
        (None, None)
    };
    EnvironmentFacts {
        cwd: cwd.to_path_buf(),
        is_repo: git_root.is_some(),
        git_root,
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        os_version: run(cwd, "uname", &["-sr"]).filter(|s| !s.is_empty()),
        shell: std::env::var("SHELL").ok().filter(|s| !s.is_empty()),
        date: opts.date.clone().unwrap_or_else(today),
        model: opts.model.clone(),
        knowledge_cutoff: opts.knowledge_cutoff.clone(),
        git,
    }
}

impl EnvironmentFacts {
    /// Renders the facts as an `<environment>` block for the system prompt.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_context::{probe_environment, ProbeOptions};
    ///
    /// let mut o = ProbeOptions::default();
    /// o.git = false;
    /// let text = probe_environment(std::path::Path::new("/tmp"), &o).render();
    /// assert!(text.starts_with("<environment>"));
    /// assert!(text.contains("Working directory: /tmp"));
    /// ```
    pub fn render(&self) -> String {
        let mut l = vec!["<environment>".to_string()];
        l.push(format!("Working directory: {}", self.cwd.display()));
        match &self.git_root {
            Some(r) => l.push(format!("Git repository: yes (root: {})", r.display())),
            None => l.push("Git repository: no".to_string()),
        }
        l.push(format!("Platform: {} ({})", self.os, self.arch));
        if let Some(v) = &self.os_version {
            l.push(format!("OS version: {v}"));
        }
        if let Some(s) = &self.shell {
            l.push(format!("Shell: {s}"));
        }
        l.push(format!("Today's date: {}", self.date));
        if let Some(m) = &self.model {
            match &self.knowledge_cutoff {
                Some(c) => l.push(format!("Model: {m} (knowledge cutoff: {c})")),
                None => l.push(format!("Model: {m}")),
            }
        }
        if let Some(g) = &self.git {
            if let Some(b) = &g.branch {
                l.push(format!("Current branch: {b}"));
            }
            if let Some(b) = &g.default_branch {
                l.push(format!("Default branch: {b}"));
            }
            if g.status.is_empty() {
                l.push("Git status: clean".to_string());
            } else {
                l.push("Git status:".to_string());
                l.extend(g.status.iter().map(|s| format!("  {s}")));
                if g.status_truncated {
                    l.push("  [... more entries omitted]".to_string());
                }
            }
            if !g.recent_commits.is_empty() {
                l.push("Recent commits:".to_string());
                l.extend(g.recent_commits.iter().map(|s| format!("  {s}")));
            }
        }
        l.push("</environment>".to_string());
        l.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_753), (2026, 10, 27));
        assert_eq!(civil_from_days(19_794), (2024, 3, 12));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    }

    #[test]
    fn today_has_iso_shape() {
        let t = today();
        assert_eq!(t.len(), 10);
        assert_eq!(&t[4..5], "-");
    }

    #[test]
    fn probes_a_git_repo() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        let git = |args: &[&str]| {
            Command::new("git")
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .current_dir(p)
                .output()
        };
        if git(&["init", "-q", "-b", "main"]).map_or(true, |o| !o.status.success()) {
            return; // git unavailable
        }
        std::fs::write(p.join("a.txt"), "x").unwrap();
        git(&["add", "."]).unwrap();
        git(&["commit", "-q", "-m", "first commit"]).unwrap();
        std::fs::write(p.join("b.txt"), "y").unwrap();

        let o = ProbeOptions {
            date: Some("2026-10-27".into()),
            model: Some("m".into()),
            ..ProbeOptions::default()
        };
        let env = probe_environment(p, &o);
        assert!(env.is_repo);
        let g = env.git.as_ref().unwrap();
        assert_eq!(g.branch.as_deref(), Some("main"));
        assert_eq!(g.default_branch.as_deref(), Some("main"));
        assert_eq!(g.status, vec!["?? b.txt"]);
        assert_eq!(g.recent_commits, vec!["first commit"]);
        let text = env.render();
        assert!(text.contains("Today's date: 2026-10-27"));
        assert!(text.contains("?? b.txt"));
    }

    #[test]
    fn non_repo_and_status_cap() {
        let dir = tempfile::tempdir().unwrap();
        let o = ProbeOptions {
            git: false,
            ..ProbeOptions::default()
        };
        let env = probe_environment(dir.path(), &o);
        assert!(!env.is_repo);
        assert!(env.render().contains("Git repository: no"));

        let facts = EnvironmentFacts {
            git: Some(GitFacts {
                status: vec!["M a".into()],
                status_truncated: true,
                ..GitFacts::default()
            }),
            ..env
        };
        assert!(facts.render().contains("more entries omitted"));
    }
}
