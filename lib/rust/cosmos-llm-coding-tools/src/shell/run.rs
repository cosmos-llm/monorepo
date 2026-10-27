//! Running one command: spawn in its own process group, merge the output,
//! stream it, and kill the whole group on timeout or interrupt. Also the
//! table of background jobs.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cosmos_llm_tool::output_limit::strip_ansi;
use cosmos_llm_tool::ProgressSink;
use tokio::io::AsyncReadExt;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use super::ir::ShellStatus;

/// Output kept in memory per run or job. Beyond this the rest is dropped.
pub(crate) const MAX_CAPTURE: usize = 32 * 1024 * 1024;
/// Output kept per background job.
const MAX_JOB_TEXT: usize = 8 * 1024 * 1024;
/// Time after the shell exits to wait for stragglers still holding the pipe.
const DRAIN_IDLE: Duration = Duration::from_millis(150);
/// Finished jobs kept for `job_output`.
const KEEP_FINISHED: usize = 32;

pub(crate) struct RunRequest {
    pub shell: PathBuf,
    pub command: String,
    pub cwd: PathBuf,
    pub timeout: Option<Duration>,
    /// Append the `pwd` trailer and report the final directory.
    pub track_cwd: bool,
    pub env: Vec<(String, String)>,
    /// SIGTERM-to-SIGKILL grace on timeout.
    pub grace: Duration,
}

pub(crate) struct RunResult {
    pub output: String,
    pub status: ShellStatus,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub new_cwd: Option<String>,
    pub capture_truncated: bool,
    pub duration: Duration,
    /// Set when the shell could not start.
    pub error: Option<String>,
}

fn store(line: &str, output: &mut String, truncated: &mut bool) {
    if output.len() + line.len() < MAX_CAPTURE {
        output.push_str(line);
        output.push('\n');
    } else {
        *truncated = true;
    }
}

/// Where lines go as they arrive.
pub(crate) enum Sink {
    Progress(ProgressSink),
    Job(Arc<Job>),
}

impl Sink {
    async fn emit(&self, line: &str) {
        match self {
            Sink::Progress(p) => p.send(format!("{}\n", strip_ansi(line))).await,
            Sink::Job(j) => j.append(&format!("{}\n", strip_ansi(line))),
        }
    }
}

#[cfg(unix)]
fn signal_group(pid: u32, sig: i32) {
    // SAFETY: kill(2) with a negative pid signals a process group; it has no
    // memory-safety preconditions.
    unsafe {
        libc::kill(-(pid as i32), sig);
    }
}

/// Kills the process group if dropped while armed, so a run abandoned by its
/// caller (a future dropped by an outer timeout) leaves nothing behind.
struct GroupGuard(Option<u32>);

impl GroupGuard {
    fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for GroupGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.0 {
            signal_group(pid, libc::SIGKILL);
        }
    }
}

/// Splits the merged stream into lines and removes the `pwd` trailer.
struct Lines {
    bytes: Vec<u8>,
    sentinel: Option<String>,
    held_blank: bool,
    after_sentinel: bool,
    cwd: Option<String>,
}

impl Lines {
    fn new(sentinel: Option<String>) -> Self {
        Self {
            bytes: Vec::new(),
            sentinel,
            held_blank: false,
            after_sentinel: false,
            cwd: None,
        }
    }

    fn line(&mut self, raw: &[u8], out: &mut Vec<String>) {
        let line = String::from_utf8_lossy(raw);
        let line = line.strip_suffix('\r').unwrap_or(&line).to_string();
        if self.after_sentinel {
            if self.cwd.is_none() {
                self.cwd = Some(line);
            }
            return;
        }
        if self.sentinel.as_deref() == Some(line.as_str()) {
            self.after_sentinel = true;
            self.held_blank = false;
            return;
        }
        if self.held_blank {
            out.push(String::new());
            self.held_blank = false;
        }
        if line.is_empty() && self.sentinel.is_some() {
            // The trailer's leading newline looks like this until the
            // sentinel arrives.
            self.held_blank = true;
        } else {
            out.push(line);
        }
    }

    fn push(&mut self, data: &[u8]) -> Vec<String> {
        self.bytes.extend_from_slice(data);
        let mut out = Vec::new();
        while let Some(i) = self.bytes.iter().position(|b| *b == b'\n') {
            let raw: Vec<u8> = self.bytes.drain(..=i).collect();
            self.line(&raw[..raw.len() - 1], &mut out);
        }
        out
    }

    fn finish(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.bytes.is_empty() {
            let raw = std::mem::take(&mut self.bytes);
            self.line(&raw, &mut out);
        }
        if self.held_blank && !self.after_sentinel {
            out.push(String::new());
        }
        self.held_blank = false;
        out
    }
}

fn sentinel() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("__COSMOS_CWD_{}_{nanos}__", std::process::id())
}

/// Finds an executable on `PATH`.
pub(crate) fn find_in_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

pub(crate) async fn run(req: RunRequest, sink: &Sink, cancel: &CancellationToken) -> RunResult {
    let start = Instant::now();
    let marker = req.track_cwd.then(sentinel);
    let script = match &marker {
        Some(m) => format!(
            "exec 2>&1\n{}\n__rc=$?; printf '\\n%s\\n' {m}; pwd; exit $__rc\n",
            req.command
        ),
        None => format!("exec 2>&1\n{}\n", req.command),
    };
    let mut cmd = tokio::process::Command::new(&req.shell);
    cmd.arg("-c")
        .arg(&script)
        .current_dir(&req.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env("TERM", "dumb")
        .env("NO_COLOR", "1")
        .env("PAGER", "cat")
        .env("GIT_PAGER", "cat")
        .env("GIT_TERMINAL_PROMPT", "0")
        .kill_on_drop(true);
    for (k, v) in &req.env {
        cmd.env(k, v);
    }
    #[cfg(unix)]
    cmd.process_group(0);

    let failed = |msg: String, start: Instant| RunResult {
        output: String::new(),
        status: ShellStatus::SpawnFailed,
        exit_code: None,
        signal: None,
        new_cwd: None,
        capture_truncated: false,
        duration: start.elapsed(),
        error: Some(msg),
    };
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return failed(
                format!("could not start {}: {e}", req.shell.display()),
                start,
            )
        }
    };
    let pid = child.id();
    let mut guard = GroupGuard(pid);
    let Some(mut stdout) = child.stdout.take() else {
        return failed("no stdout pipe".into(), start);
    };

    let mut lines = Lines::new(marker);
    let mut output = String::new();
    let mut capture_truncated = false;

    let deadline = req.timeout.map(|d| tokio::time::Instant::now() + d);
    let mut buf = vec![0u8; 16 * 1024];
    let mut exit: Option<std::process::ExitStatus> = None;
    let mut ended: Option<ShellStatus> = None;

    loop {
        let exited = exit.is_some();
        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                ended = Some(ShellStatus::Interrupted);
                break;
            }
            _ = tokio::time::sleep_until(deadline.unwrap_or_else(tokio::time::Instant::now)),
                if deadline.is_some() => {
                ended = Some(ShellStatus::TimedOut);
                break;
            }
            st = child.wait(), if !exited => {
                exit = st.ok();
            }
            n = async {
                if exited {
                    tokio::time::timeout(DRAIN_IDLE, stdout.read(&mut buf)).await.unwrap_or(Ok(0))
                } else {
                    stdout.read(&mut buf).await
                }
            } => match n {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    for l in lines.push(&buf[..n]) {
                        sink.emit(&l).await;
                        store(&l, &mut output, &mut capture_truncated);
                    }
                }
            },
        }
    }

    if let Some(status) = ended {
        let grace = if status == ShellStatus::TimedOut {
            req.grace
        } else {
            Duration::from_millis(200)
        };
        terminate(&mut child, pid, grace).await;
        // Whatever the group wrote before it died.
        while let Ok(Ok(n)) = tokio::time::timeout(DRAIN_IDLE, stdout.read(&mut buf)).await {
            if n == 0 {
                break;
            }
            for l in lines.push(&buf[..n]) {
                sink.emit(&l).await;
                store(&l, &mut output, &mut capture_truncated);
            }
        }
    }
    for l in lines.finish() {
        sink.emit(&l).await;
        store(&l, &mut output, &mut capture_truncated);
    }

    let status = match exit {
        Some(s) => Some(s),
        None if ended.is_none() => child.wait().await.ok(),
        None => child.try_wait().ok().flatten(),
    };
    if ended.is_none() {
        // Finished normally: anything left in the group was left on purpose.
        guard.disarm();
    }
    let (exit_code, signal) = match status {
        Some(s) => (s.code(), exit_signal(&s)),
        None => (None, None),
    };
    RunResult {
        output,
        status: ended.unwrap_or(ShellStatus::Exited),
        exit_code,
        signal,
        new_cwd: lines.cwd,
        capture_truncated,
        duration: start.elapsed(),
        error: None,
    }
}

#[cfg(unix)]
fn exit_signal(s: &std::process::ExitStatus) -> Option<i32> {
    std::os::unix::process::ExitStatusExt::signal(s)
}

#[cfg(not(unix))]
fn exit_signal(_: &std::process::ExitStatus) -> Option<i32> {
    None
}

/// SIGTERM the group, wait up to `grace`, then SIGKILL it.
async fn terminate(child: &mut tokio::process::Child, pid: Option<u32>, grace: Duration) {
    #[cfg(unix)]
    if let Some(pid) = pid {
        signal_group(pid, libc::SIGTERM);
        let _ = tokio::time::timeout(grace, child.wait()).await;
        // The shell may be gone while its children are not.
        signal_group(pid, libc::SIGKILL);
        let _ = child.wait().await;
        return;
    }
    let _ = (pid, grace);
    let _ = child.kill().await;
}

/// A background job. Output is read with `job_output`, never through a
/// progress sink (a sink is valid only for the call that created it).
pub(crate) struct Job {
    pub id: String,
    pub command: String,
    pub cancel: CancellationToken,
    state: Mutex<JobState>,
    notify: Notify,
}

#[derive(Default)]
struct JobState {
    text: String,
    cursor: usize,
    capped: bool,
    done: Option<JobDone>,
}

#[derive(Clone)]
pub(crate) struct JobDone {
    pub status: ShellStatus,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
}

/// A snapshot read of a job.
pub(crate) struct JobRead {
    pub text: String,
    pub next_offset: usize,
    pub done: Option<JobDone>,
    pub capped: bool,
}

impl Job {
    fn state(&self) -> std::sync::MutexGuard<'_, JobState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn append(&self, text: &str) {
        let mut s = self.state();
        if s.text.len() + text.len() <= MAX_JOB_TEXT {
            s.text.push_str(text);
        } else {
            s.capped = true;
        }
        drop(s);
        self.notify.notify_waiters();
    }

    fn finish(&self, done: JobDone) {
        self.state().done = Some(done);
        self.notify.notify_waiters();
    }

    pub fn is_running(&self) -> bool {
        self.state().done.is_none()
    }

    /// Output from `since` (or the last read position), advancing the
    /// position.
    pub fn read(&self, since: Option<usize>) -> JobRead {
        let mut s = self.state();
        let mut from = since.unwrap_or(s.cursor).min(s.text.len());
        while !s.text.is_char_boundary(from) {
            from += 1;
        }
        let text = s.text[from..].to_string();
        s.cursor = s.text.len();
        JobRead {
            text,
            next_offset: s.cursor,
            done: s.done.clone(),
            capped: s.capped,
        }
    }

    /// Waits until there is unread output, the job finished, `max` passed, or
    /// `cancel` fired.
    pub async fn wait(&self, max: Duration, cancel: &CancellationToken) {
        let deadline = tokio::time::Instant::now() + max;
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let s = self.state();
                if s.done.is_some() || s.text.len() > s.cursor {
                    return;
                }
            }
            tokio::select! {
                _ = &mut notified => {}
                _ = tokio::time::sleep_until(deadline) => return,
                _ = cancel.cancelled() => return,
            }
        }
    }

    /// Waits for the job to finish, up to `max`.
    pub async fn wait_done(&self, max: Duration) {
        let deadline = tokio::time::Instant::now() + max;
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if !self.is_running() {
                return;
            }
            tokio::select! {
                _ = &mut notified => {}
                _ = tokio::time::sleep_until(deadline) => return,
            }
        }
    }
}

/// The jobs of one session. Dropping the table kills every running job.
pub(crate) struct JobTable {
    jobs: Mutex<Vec<Arc<Job>>>,
    next: AtomicU64,
    max_running: usize,
}

impl JobTable {
    pub fn new(max_running: usize) -> Self {
        Self {
            jobs: Mutex::new(Vec::new()),
            next: AtomicU64::new(1),
            max_running,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Arc<Job>>> {
        self.jobs.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn get(&self, id: &str) -> Option<Arc<Job>> {
        self.lock().iter().find(|j| j.id == id).cloned()
    }

    pub fn running(&self) -> usize {
        self.lock().iter().filter(|j| j.is_running()).count()
    }

    /// Starts `req` as a job.
    ///
    /// # Errors
    ///
    /// A message for the model if too many jobs are running.
    pub fn start(&self, req: RunRequest) -> Result<Arc<Job>, String> {
        let mut jobs = self.lock();
        let running = jobs.iter().filter(|j| j.is_running()).count();
        if running >= self.max_running {
            return Err(format!(
                "{running} background jobs are already running (the limit is {}). \
                 Stop one with job_kill first.",
                self.max_running
            ));
        }
        let finished = jobs.iter().filter(|j| !j.is_running()).count();
        if finished > KEEP_FINISHED {
            if let Some(i) = jobs.iter().position(|j| !j.is_running()) {
                jobs.remove(i);
            }
        }
        let id = format!("job_{}", self.next.fetch_add(1, Ordering::Relaxed));
        let job = Arc::new(Job {
            id,
            command: req.command.clone(),
            cancel: CancellationToken::new(),
            state: Mutex::new(JobState::default()),
            notify: Notify::new(),
        });
        jobs.push(Arc::clone(&job));
        drop(jobs);
        let runner = Arc::clone(&job);
        tokio::spawn(async move {
            let sink = Sink::Job(Arc::clone(&runner));
            let res = run(req, &sink, &runner.cancel).await;
            if let Some(e) = &res.error {
                runner.append(&format!("{e}\n"));
            }
            runner.finish(JobDone {
                status: res.status,
                exit_code: res.exit_code,
                signal: res.signal,
            });
        });
        Ok(job)
    }
}

impl Drop for JobTable {
    fn drop(&mut self) {
        // Cancelling ends each run gracefully; if the runtime is already
        // gone, dropping the run's future kills its process group instead.
        for j in self.lock().iter() {
            j.cancel.cancel();
        }
    }
}

/// Resolves the shell to run: the configured one, else `bash`, else `sh`.
pub(crate) fn resolve_shell(configured: Option<&Path>) -> Option<PathBuf> {
    configured
        .map(Path::to_path_buf)
        .or_else(|| find_in_path("bash"))
        .or_else(|| find_in_path("sh"))
}
