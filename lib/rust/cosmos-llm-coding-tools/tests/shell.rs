//! The shell tools against a real shell. Unix only.
#![cfg(unix)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use cosmos_llm_coding_tools::shell::{ShellConfig, ShellTools};
use cosmos_llm_coding_tools::{CodingTools, DiskFs, FileTracker};
use cosmos_llm_tool::output_limit::SpillStore;
use cosmos_llm_tool::{Executor, OutputLimit, Registry, ToolContext, ToolDefinition};
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

struct Env {
    dir: tempfile::TempDir,
    tools: CodingTools,
    bash: ToolDefinition,
    job_output: ToolDefinition,
    job_kill: ToolDefinition,
}

fn env_with(config: ShellConfig) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let tools =
        CodingTools::new(Arc::new(DiskFs::new(dir.path()).unwrap())).with_shell_config(config);
    let find = |n: &str| tools.all().into_iter().find(|t| t.name == n).unwrap();
    Env {
        bash: find("bash"),
        job_output: find("job_output"),
        job_kill: find("job_kill"),
        dir,
        tools,
    }
}

fn env() -> Env {
    env_with(ShellConfig::new())
}

async fn call(tool: &ToolDefinition, params: Value) -> Result<Value, String> {
    Executor::execute_with(tool, params, ToolContext::detached("c1"))
        .await
        .map_err(|e| e.to_string())
}

async fn bash(env: &Env, command: &str) -> String {
    match call(&env.bash, json!({ "command": command })).await {
        Ok(v) => v.as_str().unwrap().to_string(),
        Err(e) => panic!("bash failed: {e}"),
    }
}

fn pid_alive(pid: i32) -> bool {
    // Signal 0 only checks existence; a zombie still counts, so also look at
    // /proc state where it exists.
    let alive = kill_zero(pid);
    if !alive {
        return false;
    }
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(s) => !s.rsplit(") ").next().unwrap_or("").starts_with('Z'),
        Err(_) => false,
    }
}

fn kill_zero(pid: i32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

async fn wait_gone(pid: i32) {
    for _ in 0..50 {
        if !pid_alive(pid) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("process {pid} survived");
}

#[test]
fn registered_with_the_coding_tools() {
    let e = env();
    let mut registry = Registry::new();
    e.tools.register_all(&mut registry);
    for n in ["bash", "job_output", "job_kill"] {
        assert!(registry.registered(n), "{n}");
    }
    assert_eq!(registry.count(), 9);
    assert_eq!(e.tools.file_tools().len(), 6);
}

#[test]
fn hints_and_preview() {
    let e = env();
    let input = json!({ "command": "echo hi > /etc/x; ls ../up", "description": "d" });
    let hints = e.bash.call_hints(&input);
    assert!(hints.destructive);
    let paths: Vec<_> = hints.paths.iter().map(|p| p.to_str().unwrap()).collect();
    assert_eq!(paths, ["/etc/x", "../up"]);
    let preview = e.bash.call_preview(&input).unwrap();
    assert_eq!(preview["command"], "echo hi > /etc/x; ls ../up");
    assert_eq!(preview["cwd"], ".");
    assert_eq!(preview["description"], "d");
    assert!(e.bash.output_limit.is_some());
}

#[tokio::test]
async fn echo_and_exit_codes() {
    let e = env();
    let out = bash(&e, "echo hi").await;
    assert!(out.starts_with("hi\n"), "{out}");
    assert!(out.contains("[exit code: 0"), "{out}");
    // Non-zero is a result, not an error.
    let out = bash(&e, "echo oops >&2; exit 3").await;
    assert!(out.contains("oops"), "{out}");
    assert!(out.contains("[exit code: 3"), "{out}");
    assert!(bash(&e, "true").await.contains("(no output)"));
}

#[tokio::test]
async fn stdout_and_stderr_merge_in_order() {
    let e = env();
    let out = bash(&e, "echo a; echo b >&2; echo c").await;
    assert!(out.starts_with("a\nb\nc\n"), "{out}");
}

#[tokio::test]
async fn working_directory_carries_over() {
    let e = env();
    std::fs::create_dir(e.dir.path().join("sub")).unwrap();
    let out = bash(&e, "cd sub && pwd").await;
    assert!(out.contains("sub\n"), "{out}");
    assert!(out.contains("Working directory is now sub"), "{out}");
    let out = bash(&e, "pwd").await;
    assert!(
        out.starts_with(&format!(
            "{}\n",
            e.dir.path().join("sub").canonicalize().unwrap().display()
        )),
        "{out}"
    );
    assert!(e.tools.shell().state().cwd.ends_with("sub"));
}

#[tokio::test]
async fn leaving_the_roots_resets_the_directory() {
    let e = env();
    std::fs::create_dir(e.dir.path().join("sub")).unwrap();
    bash(&e, "cd sub").await;
    let out = bash(&e, "cd /").await;
    assert!(out.contains("reset to"), "{out}");
    assert_eq!(
        e.tools.shell().state().cwd,
        e.dir.path().canonicalize().unwrap()
    );
    let out = bash(&e, "pwd").await;
    assert!(out.starts_with(&e.dir.path().canonicalize().unwrap().display().to_string()));
}

#[tokio::test]
async fn trailer_does_not_leak_into_output() {
    let e = env();
    let out = bash(&e, "printf 'no newline'").await;
    assert!(out.starts_with("no newline\n[exit code"), "{out}");
    assert!(!out.contains("__COSMOS"), "{out}");
    let out = bash(&e, "echo a; echo; echo b").await;
    assert!(out.starts_with("a\n\nb\n[exit"), "{out}");
}

#[tokio::test]
async fn timeout_kills_the_process_group() {
    let e = env();
    let started = Instant::now();
    let err = call(
        &e.bash,
        json!({ "command": "sleep 31 & echo $! > child.pid; echo started; sleep 30", "timeout_ms": 300 }),
    )
    .await
    .unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(err.contains("timed out after 300ms"), "{err}");
    assert!(err.contains("started"), "partial output is kept: {err}");
    let pid: i32 = std::fs::read_to_string(e.dir.path().join("child.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    wait_gone(pid).await;
}

#[tokio::test]
async fn interrupt_kills_the_process_group_promptly() {
    let e = env();
    let token = CancellationToken::new();
    let mut ctx = ToolContext::detached("c1");
    ctx.cancel = token.clone();
    let canceller = {
        let t = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(400)).await;
            t.cancel();
        })
    };
    let started = Instant::now();
    let err = Executor::execute_with(
        &e.bash,
        json!({ "command": "sleep 32 & echo $! > a.pid; sleep 33 & echo $! > b.pid; echo up; wait" }),
        ctx,
    )
    .await
    .unwrap_err()
    .to_string();
    canceller.await.unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert!(err.contains("interrupted"), "{err}");
    assert!(err.contains("up"), "{err}");
    for f in ["a.pid", "b.pid"] {
        let pid: i32 = std::fs::read_to_string(e.dir.path().join(f))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        wait_gone(pid).await;
    }
}

#[tokio::test]
async fn background_job_output_and_exit() {
    let e = env();
    let started = call(
        &e.bash,
        json!({ "command": "sleep 0.2; echo done", "run_in_background": true }),
    )
    .await
    .unwrap();
    let text = started.as_str().unwrap();
    assert!(text.contains("job_1"), "{text}");
    let mut output = String::new();
    for _ in 0..50 {
        let r = call(&e.job_output, json!({ "job_id": "job_1", "wait_ms": 500 }))
            .await
            .unwrap();
        output.push_str(r["output"].as_str().unwrap());
        if r["running"] == false {
            assert_eq!(r["exit_code"], 0);
            break;
        }
    }
    assert!(output.contains("done"), "{output}");
    // Reading again returns only what is new.
    let r = call(&e.job_output, json!({ "job_id": "job_1" }))
        .await
        .unwrap();
    assert_eq!(r["output"], "");
    let r = call(&e.job_output, json!({ "job_id": "job_1", "since": 0 }))
        .await
        .unwrap();
    assert!(r["output"].as_str().unwrap().contains("done"));
    let r = call(&e.job_output, json!({ "job_id": "nope" }))
        .await
        .unwrap();
    assert_eq!(r["success"], false);
}

#[tokio::test]
async fn job_kill_stops_the_group() {
    let e = env();
    call(
        &e.bash,
        json!({ "command": "sleep 34 & echo $! > j.pid; echo ready; wait", "run_in_background": true }),
    )
    .await
    .unwrap();
    let r = call(&e.job_output, json!({ "job_id": "job_1", "wait_ms": 2000 }))
        .await
        .unwrap();
    assert_eq!(r["running"], true);
    assert_eq!(e.tools.shell().running_jobs(), 1);
    let r = call(&e.job_kill, json!({ "job_id": "job_1" }))
        .await
        .unwrap();
    assert_eq!(r["success"], true);
    assert_eq!(r["running"], false);
    assert_eq!(e.tools.shell().running_jobs(), 0);
    let pid: i32 = std::fs::read_to_string(e.dir.path().join("j.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    wait_gone(pid).await;
}

#[tokio::test]
async fn job_cap_is_enforced() {
    let e = env_with(ShellConfig::new().max_jobs(1));
    let run = json!({ "command": "sleep 35", "run_in_background": true });
    call(&e.bash, run.clone()).await.unwrap();
    let err = call(&e.bash, run).await.unwrap_err();
    assert!(err.contains("limit is 1"), "{err}");
    call(&e.job_kill, json!({ "job_id": "job_1" }))
        .await
        .unwrap();
}

#[tokio::test]
async fn large_output_is_truncated_and_spilled() {
    let e = env();
    let out = bash(&e, "seq 1 200000").await;
    assert!(out.len() > 1_000_000, "{}", out.len());
    let spill_dir = tempfile::tempdir().unwrap();
    let store = SpillStore::new(spill_dir.path());
    let limited = Executor::limit_result(
        &e.bash,
        &Value::String(out.clone()),
        &OutputLimit::new(),
        Some(&store),
    );
    assert!(limited.truncated);
    assert!(limited.text.len() < 31_000);
    // Head and tail survive: the exit footer is at the end.
    assert!(limited.text.contains("[exit code: 0"), "{}", limited.text);
    let spilled = std::fs::read_to_string(limited.spill_path.unwrap()).unwrap();
    assert_eq!(spilled, out);
}

#[tokio::test]
async fn shell_commands_unverify_tracked_files() {
    let e = env();
    std::fs::write(e.dir.path().join("f.txt"), "one\n").unwrap();
    let read = e
        .tools
        .all()
        .into_iter()
        .find(|t| t.name == "read")
        .unwrap();
    read.call(&json!({ "file_path": "f.txt" })).unwrap();
    let edit = e
        .tools
        .all()
        .into_iter()
        .find(|t| t.name == "edit")
        .unwrap();
    bash(&e, "echo two > f.txt").await;
    let r = edit
        .call(&json!({ "file_path": "f.txt", "old_string": "two", "new_string": "3" }))
        .unwrap();
    assert_eq!(r["success"], false, "{r}");
}

#[tokio::test]
async fn bad_input() {
    let e = env();
    assert!(call(&e.bash, json!({ "command": "  " })).await.is_err());
    assert!(call(&e.bash, json!({ "command": "ls", "timeout_ms": 0 }))
        .await
        .is_err());
    let bad = env_with(ShellConfig::new().shell("/nonexistent/shell"));
    let err = call(&bad.bash, json!({ "command": "ls" }))
        .await
        .unwrap_err();
    assert!(err.contains("could not start"), "{err}");
}

#[tokio::test]
async fn env_is_not_carried_but_is_quiet() {
    let e = env();
    bash(&e, "export FOO=1").await;
    let out = bash(&e, "echo \"[$FOO] $TERM $PAGER\"").await;
    assert!(out.starts_with("[] dumb cat\n"), "{out}");
}

#[test]
fn standalone_construction() {
    let dir = tempfile::tempdir().unwrap();
    let t = ShellTools::new(
        Arc::new(DiskFs::new(dir.path()).unwrap()),
        FileTracker::shared(),
    );
    assert_eq!(t.all().len(), 3);
}
