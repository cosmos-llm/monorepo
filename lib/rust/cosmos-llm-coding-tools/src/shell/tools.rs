//! The `bash`, `job_output` and `job_kill` tool definitions.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cosmos_llm_tool::fs::ToolFs;
use cosmos_llm_tool::output_limit::{clean_terminal_output, limit_output, OutputLimit, Truncate};
use cosmos_llm_tool::{ParameterType, ToolContext, ToolDefinition, ToolHints};
use serde_json::{json, Value};

use super::ir::{ShellRun, ShellStatus};
use super::parse::parse_shell;
use super::run::{resolve_shell, run, JobDone, JobTable, RunRequest, Sink};
use crate::tracker::SharedTracker;
use crate::DiskFs;

/// Settings for the shell tools.
///
/// # Examples
///
/// ```rust
/// use std::time::Duration;
/// use cosmos_llm_coding_tools::shell::ShellConfig;
///
/// let cfg = ShellConfig::new().max_jobs(2).default_timeout(Duration::from_secs(30));
/// assert_eq!(cfg.max_jobs, 2);
/// ```
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ShellConfig {
    /// The shell to run (`-c`). Default: `bash` from `PATH`, else `sh`.
    pub shell: Option<PathBuf>,
    /// Concurrent background jobs. Default 8.
    pub max_jobs: usize,
    /// Timeout when the call gives none. Default 120 s.
    pub default_timeout: Duration,
    /// Largest timeout a call may ask for. Default 600 s.
    pub max_timeout: Duration,
    /// Time between SIGTERM and SIGKILL on timeout. Default 2 s.
    pub kill_grace: Duration,
    /// Extra environment for every command.
    pub env: Vec<(String, String)>,
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            shell: None,
            max_jobs: 8,
            default_timeout: Duration::from_millis(120_000),
            max_timeout: Duration::from_millis(600_000),
            kill_grace: Duration::from_secs(2),
            env: Vec::new(),
        }
    }
}

impl ShellConfig {
    /// The defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the shell binary.
    #[must_use]
    pub fn shell(mut self, shell: impl Into<PathBuf>) -> Self {
        self.shell = Some(shell.into());
        self
    }

    /// Sets the background job cap.
    #[must_use]
    pub fn max_jobs(mut self, n: usize) -> Self {
        self.max_jobs = n;
        self
    }

    /// Sets the default timeout.
    #[must_use]
    pub fn default_timeout(mut self, d: Duration) -> Self {
        self.default_timeout = d;
        self
    }

    /// Sets the largest timeout a call may request.
    #[must_use]
    pub fn max_timeout(mut self, d: Duration) -> Self {
        self.max_timeout = d;
        self
    }

    /// Sets the SIGTERM-to-SIGKILL grace period.
    #[must_use]
    pub fn kill_grace(mut self, d: Duration) -> Self {
        self.kill_grace = d;
        self
    }

    /// Adds an environment variable for every command.
    #[must_use]
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }
}

/// What carries over between `bash` calls: the working directory.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ShellState {
    /// Where the next command starts.
    pub cwd: PathBuf,
}

struct Inner {
    fs: Arc<DiskFs>,
    tracker: SharedTracker,
    state: Mutex<ShellState>,
    jobs: JobTable,
    config: ShellConfig,
}

/// The shell tools bound to one working tree, one tracker and one job table.
///
/// Cloning shares all three. Jobs are killed when the last clone (and the
/// tool definitions made from it) is dropped.
///
/// # Examples
///
/// ```rust
/// use std::sync::Arc;
/// use cosmos_llm_coding_tools::shell::ShellTools;
/// use cosmos_llm_coding_tools::{DiskFs, FileTracker};
///
/// let dir = tempfile::tempdir().unwrap();
/// let fs = Arc::new(DiskFs::new(dir.path()).unwrap());
/// let shell = ShellTools::new(fs, FileTracker::shared());
/// let names: Vec<_> = shell.all().into_iter().map(|t| t.name).collect();
/// assert_eq!(names, ["bash", "job_output", "job_kill"]);
/// ```
#[derive(Clone)]
pub struct ShellTools {
    inner: Arc<Inner>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn shell_limit() -> OutputLimit {
    OutputLimit::new()
        .max_bytes(30_000)
        .strategy(Truncate::HeadTail { head_ratio: 0.3 })
}

fn shown(fs: &DiskFs, path: &Path) -> String {
    let rel = fs.relativize(path);
    if rel.as_os_str().is_empty() {
        ".".into()
    } else {
        rel.to_string_lossy().into_owned()
    }
}

/// Paths a command visibly touches outside plain arguments: redirect targets
/// and arguments that look like absolute, home or parent-relative paths.
/// Hints only; the policy classifies the command itself.
fn command_paths(command: &str) -> Vec<String> {
    let Ok(ir) = parse_shell(command) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for c in ir.simple_commands() {
        for r in &c.redirects {
            if let Some(t) = r.target.as_literal() {
                if r.op.writes() && !t.is_empty() && t.parse::<u32>().is_err() && t != "-" {
                    out.push(t.to_string());
                }
            }
        }
        for w in c.argv.iter().skip(1) {
            if let Some(t) = w.as_literal() {
                if t.starts_with('/') || t.starts_with("~/") || t.starts_with("../") || t == ".." {
                    out.push(t.to_string());
                }
            }
        }
    }
    out
}

impl ShellTools {
    /// Binds the tools to `fs` and `tracker` with default settings.
    pub fn new(fs: Arc<DiskFs>, tracker: SharedTracker) -> Self {
        Self::with_config(fs, tracker, ShellConfig::default())
    }

    /// Binds the tools with explicit settings.
    pub fn with_config(fs: Arc<DiskFs>, tracker: SharedTracker, config: ShellConfig) -> Self {
        let cwd = fs.root().to_path_buf();
        Self {
            inner: Arc::new(Inner {
                fs,
                tracker,
                state: Mutex::new(ShellState { cwd }),
                jobs: JobTable::new(config.max_jobs),
                config,
            }),
        }
    }

    /// The tracked shell state.
    pub fn state(&self) -> ShellState {
        lock(&self.inner.state).clone()
    }

    /// Number of background jobs still running.
    pub fn running_jobs(&self) -> usize {
        self.inner.jobs.running()
    }

    /// `bash`, `job_output` and `job_kill`.
    pub fn all(&self) -> Vec<ToolDefinition> {
        vec![
            self.bash_tool(),
            self.job_output_tool(),
            self.job_kill_tool(),
        ]
    }

    /// The `bash` tool.
    pub fn bash_tool(&self) -> ToolDefinition {
        let this = self.clone();
        let for_preview = self.clone();
        ToolDefinition::new("bash")
            .description(
                "Run a shell command with bash. Each call is a fresh shell: the working \
                 directory carries over between calls (cd works), but environment \
                 variables do not. Stdout and stderr are merged. A non-zero exit code is \
                 reported, not treated as an error; a timeout is. Output is truncated in \
                 the middle if long; the full text is saved to a file you can read. \
                 Commands cannot read input. Set run_in_background for a server or other \
                 long-running process, then use job_output and job_kill.",
            )
            .param("command", ParameterType::String, true, "The command to run")
            .param(
                "timeout_ms",
                ParameterType::Integer,
                false,
                "Timeout in milliseconds (default 120000, maximum 600000). On timeout \
                 the whole process group is killed.",
            )
            .param(
                "description",
                ParameterType::String,
                false,
                "A short label for what the command does, shown to the user",
            )
            .param(
                "run_in_background",
                ParameterType::Boolean,
                false,
                "Start the command as a background job and return its job id at once",
            )
            .destructive(true)
            .hints(|input| {
                ToolHints::new()
                    .destructive(true)
                    .paths(command_paths(input["command"].as_str().unwrap_or("")))
            })
            .preview(move |input| {
                Some(json!({
                    "command": input["command"].as_str()?,
                    "cwd": shown(&for_preview.inner.fs, &lock(&for_preview.inner.state).cwd),
                    "description": input["description"],
                }))
            })
            .output_limit(shell_limit())
            .context_handler(move |params, ctx| {
                let this = this.clone();
                Box::pin(async move { this.run_bash(params, ctx).await })
            })
    }

    /// The `job_output` tool.
    pub fn job_output_tool(&self) -> ToolDefinition {
        let this = self.clone();
        ToolDefinition::new("job_output")
            .read_only(true)
            .description(
                "Read output from a background job started with bash. Returns the output \
                 written since the previous call (or since the byte offset `since`), \
                 whether the job is still running, and the exit code once it ends. \
                 `wait_ms` waits up to that long for new output or for the job to end.",
            )
            .param("job_id", ParameterType::String, true, "Id returned by bash")
            .param(
                "since",
                ParameterType::Integer,
                false,
                "Byte offset to read from; default is where the last read stopped",
            )
            .param(
                "wait_ms",
                ParameterType::Integer,
                false,
                "Wait up to this long (max 30000) for new output",
            )
            .output_limit(shell_limit())
            .context_handler(move |params, ctx| {
                let this = this.clone();
                Box::pin(async move {
                    let id = params["job_id"].as_str().ok_or("job_id must be a string")?;
                    let Some(job) = this.inner.jobs.get(id) else {
                        return Ok(no_such_job(id));
                    };
                    let since = params["since"].as_u64().map(|n| n as usize);
                    let wait = params["wait_ms"].as_u64().unwrap_or(0).min(30_000);
                    if wait > 0 {
                        job.wait(Duration::from_millis(wait), &ctx.cancel).await;
                    }
                    let read = job.read(since);
                    let mut v = json!({
                        "success": true,
                        "job_id": job.id,
                        "command": job.command,
                        "running": read.done.is_none(),
                        "output": clean_terminal_output(&read.text),
                        "next_offset": read.next_offset,
                    });
                    add_done(&mut v, read.done.as_ref());
                    if read.capped {
                        v["note"] =
                            json!("The job wrote more than 8 MiB; later output was dropped.");
                    }
                    Ok(v)
                })
            })
    }

    /// The `job_kill` tool.
    pub fn job_kill_tool(&self) -> ToolDefinition {
        let this = self.clone();
        ToolDefinition::new("job_kill")
            .destructive(true)
            .description("Stop a background job and its child processes.")
            .param("job_id", ParameterType::String, true, "Id returned by bash")
            .async_handler(move |params| {
                let this = this.clone();
                Box::pin(async move {
                    let id = params["job_id"].as_str().ok_or("job_id must be a string")?;
                    let Some(job) = this.inner.jobs.get(id) else {
                        return Ok(no_such_job(id));
                    };
                    let was_running = job.is_running();
                    job.cancel.cancel();
                    job.wait_done(Duration::from_secs(5)).await;
                    let read = job.read(Some(usize::MAX));
                    let mut v = json!({
                        "success": true,
                        "job_id": job.id,
                        "was_running": was_running,
                        "running": read.done.is_none(),
                    });
                    add_done(&mut v, read.done.as_ref());
                    Ok(v)
                })
            })
    }

    async fn run_bash(&self, params: Value, ctx: ToolContext) -> Result<Value, String> {
        let inner = &self.inner;
        let command = params["command"]
            .as_str()
            .ok_or("command must be a string")?
            .to_string();
        if command.trim().is_empty() {
            return Err("command must not be empty".into());
        }
        let description = params["description"].as_str().map(str::to_string);
        let background = params["run_in_background"].as_bool().unwrap_or(false);
        let cfg = &inner.config;
        let timeout = match params["timeout_ms"].as_i64() {
            Some(ms) if ms <= 0 => return Err("timeout_ms must be positive".into()),
            Some(ms) => Duration::from_millis(ms as u64).min(cfg.max_timeout),
            None => cfg.default_timeout,
        };
        let Some(shell) = resolve_shell(cfg.shell.as_deref()) else {
            return Err(
                "no shell found: install bash or sh, or configure ShellConfig::shell".into(),
            );
        };

        let mut notes = Vec::new();
        let mut cwd = lock(&inner.state).cwd.clone();
        if !cwd.is_dir() {
            let root = inner.fs.root().to_path_buf();
            notes.push(format!(
                "The working directory {} no longer exists; using {}.",
                cwd.display(),
                root.display()
            ));
            cwd = root;
            lock(&inner.state).cwd = cwd.clone();
        }
        let shown_cwd = shown(&inner.fs, &cwd);
        let req = RunRequest {
            shell,
            command: command.clone(),
            cwd: cwd.clone(),
            timeout: (!background).then_some(timeout),
            track_cwd: !background,
            env: cfg.env.clone(),
            grace: cfg.kill_grace,
        };
        let mut run_ir = ShellRun {
            command: command.clone(),
            description,
            cwd: shown_cwd,
            status: ShellStatus::Exited,
            exit_code: None,
            signal: None,
            duration_ms: 0,
            job_id: None,
            new_cwd: None,
            output_bytes: 0,
            capture_truncated: false,
        };

        if background {
            let mut req = req;
            // A job runs until killed unless the call asked for a timeout.
            req.timeout = params["timeout_ms"].as_i64().map(|_| timeout);
            let job = inner.jobs.start(req)?;
            lock(&inner.tracker).mark_all_unverified();
            run_ir.status = ShellStatus::Background;
            run_ir.job_id = Some(job.id.clone());
            ctx.set_display(to_value(&run_ir));
            return Ok(Value::String(format!(
                "Started background job {id}. Read its output with job_output \
                 {{\"job_id\": \"{id}\"}} and stop it with job_kill.",
                id = job.id
            )));
        }

        let res = run(req, &Sink::Progress(ctx.progress.clone()), &ctx.cancel).await;
        // Shell commands can change any file; per-file tracking is not possible.
        lock(&inner.tracker).mark_all_unverified();

        run_ir.status = res.status;
        run_ir.exit_code = res.exit_code;
        run_ir.signal = res.signal;
        run_ir.duration_ms = res.duration.as_millis() as u64;
        run_ir.capture_truncated = res.capture_truncated;
        let output = clean_terminal_output(&res.output);
        run_ir.output_bytes = output.len();

        if let Some(e) = res.error {
            ctx.set_display(to_value(&{
                let mut r = run_ir;
                r.status = ShellStatus::SpawnFailed;
                r
            }));
            return Err(e);
        }

        if let Some(reported) = &res.new_cwd {
            let new = Path::new(reported)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(reported));
            if inner.fs.allowed_roots().any(|r| new.starts_with(r)) {
                if new != cwd {
                    run_ir.new_cwd = Some(shown(&inner.fs, &new));
                    notes.push(format!(
                        "Working directory is now {}.",
                        shown(&inner.fs, &new)
                    ));
                }
                lock(&inner.state).cwd = new;
            } else {
                let root = inner.fs.root().to_path_buf();
                notes.push(format!(
                    "The command ended in {reported}, which is outside the allowed \
                     directories. The working directory was reset to {}.",
                    root.display()
                ));
                run_ir.new_cwd = Some(shown(&inner.fs, &root));
                lock(&inner.state).cwd = root;
            }
        }

        let secs = res.duration.as_secs_f64();
        let mut text = output;
        if text.is_empty() {
            text.push_str("(no output)\n");
        } else if !text.ends_with('\n') {
            text.push('\n');
        }
        if res.capture_truncated {
            text.push_str("[output exceeded the capture limit; the middle was dropped]\n");
        }
        for n in &notes {
            text.push_str(&format!("[{n}]\n"));
        }
        let status_line = match res.status {
            ShellStatus::TimedOut => format!(
                "[timed out after {}ms; the process group was killed. Wall time {secs:.2}s]",
                timeout.as_millis()
            ),
            ShellStatus::Interrupted => {
                format!("[interrupted; the process group was killed. Wall time {secs:.2}s]")
            }
            _ => match (res.exit_code, res.signal) {
                (Some(c), _) => format!("[exit code: {c}, wall time: {secs:.2}s]"),
                (None, Some(s)) => format!("[killed by signal {s}, wall time: {secs:.2}s]"),
                _ => format!("[exited, wall time: {secs:.2}s]"),
            },
        };
        text.push_str(&status_line);
        ctx.set_display(to_value(&run_ir));

        match res.status {
            // A timeout is a failure; the partial output rides along. The
            // loop does not limit errors, so do it here.
            ShellStatus::TimedOut | ShellStatus::Interrupted => {
                Err(limit_output(&text, &shell_limit(), None).text)
            }
            _ => Ok(Value::String(text)),
        }
    }
}

fn to_value(run: &ShellRun) -> Value {
    serde_json::to_value(run).unwrap_or(Value::Null)
}

fn no_such_job(id: &str) -> Value {
    json!({ "success": false, "error": format!("no job with id {id}") })
}

fn add_done(v: &mut Value, done: Option<&JobDone>) {
    if let Some(d) = done {
        v["status"] = json!(d.status);
        if let Some(c) = d.exit_code {
            v["exit_code"] = json!(c);
        }
        if let Some(s) = d.signal {
            v["signal"] = json!(s);
        }
    }
}
