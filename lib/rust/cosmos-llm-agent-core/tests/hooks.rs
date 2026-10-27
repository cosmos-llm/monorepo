//! Hooks (230): the five events, the Claude Code contract, and trust.

mod common;

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use common::*;
use cosmos_llm::providers::mock::MockProvider;
use cosmos_llm::CancellationToken;
use cosmos_llm_agent_core::policy::{ApprovalMode, PolicySpec};
use cosmos_llm_agent_core::{
    Core, CoreConfig, EvalSnapshot, Event, HookEvent, HookPayload, HookRunner, HookSpec, HookTrust,
    LoadOptions, LoadedHooks, PayloadFormat,
};
use cosmos_llm_tool::{AgentEvent, ParameterType, Registry, StopReason, ToolDefinition};
use serde_json::{json, Value};

fn full_auto(cwd: &Path, sess: &Path) -> CoreConfig {
    let mut c = config(cwd, sess);
    c.permissions = PolicySpec::new(ApprovalMode::FullAuto);
    c
}

/// An `echo` tool that counts its runs.
fn counting_registry() -> (Registry, Arc<AtomicUsize>) {
    let runs = Arc::new(AtomicUsize::new(0));
    let r = Arc::clone(&runs);
    let mut reg = Registry::new();
    reg.register(
        ToolDefinition::new("echo")
            .description("echoes")
            .param("text", ParameterType::String, true, "text")
            .param("file_path", ParameterType::String, false, "a path")
            .handler(move |p| {
                r.fetch_add(1, Ordering::SeqCst);
                Ok(json!(p["text"].as_str().unwrap_or("")))
            }),
    );
    (reg, runs)
}

fn tool_result(events: &[Event]) -> cosmos_llm_tool::ToolOutcome {
    events
        .iter()
        .find_map(|e| match e {
            Event::Agent(AgentEvent::ToolFinished { result, .. }) => Some(result.clone()),
            _ => None,
        })
        .expect("a tool finished")
}

async fn run_with_hooks(
    hooks: Vec<HookSpec>,
    mock: MockProvider,
) -> (Vec<Event>, Arc<AtomicUsize>, Arc<Recording>) {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (reg, runs) = counting_registry();
    let rec = completer(mock);
    let (h, mut rx) = Core::builder(full_auto(cwd.path(), sess.path()))
        .completer(rec.clone())
        .tools(reg)
        .hooks(hooks)
        .start()
        .unwrap();
    h.user_text("go").await.unwrap();
    let ev = until(&mut rx, run_finished).await;
    // Keep the tempdirs alive until the run is done.
    drop((cwd, sess));
    (ev, runs, rec)
}

#[tokio::test]
async fn pre_tool_use_exit_2_blocks_the_tool_and_shows_the_reason() {
    let hook = HookSpec::new(
        HookEvent::PreToolUse,
        "echo 'migrations/ is off limits' >&2; exit 2",
    )
    .matcher("echo|write");
    let mock = MockProvider::new()
        .respond_with_tool_call("echo", json!({"text": "hi"}))
        .respond("ok");
    let (ev, runs, rec) = run_with_hooks(vec![hook], mock).await;
    assert_eq!(runs.load(Ordering::SeqCst), 0, "the tool must not run");
    let result = tool_result(&ev);
    assert!(result.is_error);
    assert!(
        result.as_text().contains("migrations/ is off limits"),
        "{result:?}"
    );
    // The model saw the reason.
    let last = rec.last_request().unwrap();
    assert!(last
        .messages
        .iter()
        .any(|m| m.text().contains("migrations/ is off limits")
            || format!("{:?}", m.content).contains("migrations/ is off limits")));
}

#[tokio::test]
async fn pre_tool_use_matcher_leaves_other_tools_alone() {
    let hook = HookSpec::new(HookEvent::PreToolUse, "exit 2").matcher("write");
    let mock = MockProvider::new()
        .respond_with_tool_call("echo", json!({"text": "hi"}))
        .respond("ok");
    let (ev, runs, _) = run_with_hooks(vec![hook], mock).await;
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    assert_eq!(tool_result(&ev).as_text(), "hi");
}

#[tokio::test]
async fn pre_tool_use_can_rewrite_the_input() {
    let hook = HookSpec::new(
        HookEvent::PreToolUse,
        r#"cat >/dev/null; echo '{"updated_input":{"text":"rewritten"}}'"#,
    );
    let mock = MockProvider::new()
        .respond_with_tool_call("echo", json!({"text": "original"}))
        .respond("ok");
    let (ev, _, _) = run_with_hooks(vec![hook], mock).await;
    assert_eq!(tool_result(&ev).as_text(), "rewritten");
}

#[tokio::test]
async fn post_tool_use_context_is_appended_to_the_result() {
    let hook = HookSpec::new(
        HookEvent::PostToolUse,
        r#"cat >/dev/null; echo '{"additional_context":"formatted 1 file"}'"#,
    )
    .matcher("echo");
    let mock = MockProvider::new()
        .respond_with_tool_call("echo", json!({"text": "hi"}))
        .respond("ok");
    let (ev, _, _) = run_with_hooks(vec![hook], mock).await;
    let text = tool_result(&ev).as_text();
    assert!(text.starts_with("hi"), "{text}");
    assert!(text.contains("formatted 1 file"), "{text}");
}

#[tokio::test]
async fn hook_stdin_is_claude_code_compatible_json_by_default() {
    let out = tempfile::tempdir().unwrap();
    let dump = out.path().join("payload.json");
    let envdump = out.path().join("env.txt");
    let hook = HookSpec::new(
        HookEvent::PostToolUse,
        format!(
            "cat > {}; echo \"$COSMOS_EVENT|$COSMOS_TOOL_NAME|$COSMOS_FILE_PATHS\" > {}",
            dump.display(),
            envdump.display()
        ),
    );
    let mock = MockProvider::new()
        .respond_with_tool_call("echo", json!({"text": "hi", "file_path": "src/a.rs"}))
        .respond("ok");
    run_with_hooks(vec![hook], mock).await;
    let v: Value = serde_json::from_str(&std::fs::read_to_string(dump).unwrap()).unwrap();
    assert_eq!(v["hook_event_name"], "PostToolUse");
    assert_eq!(v["event"], "post_tool_use");
    assert_eq!(v["tool_name"], "echo");
    assert_eq!(v["tool"], "echo");
    assert_eq!(v["tool_input"]["file_path"], "src/a.rs");
    assert_eq!(v["tool_response"], "hi");
    assert!(v["session_id"].as_str().is_some_and(|s| !s.is_empty()));
    assert!(v["cwd"].as_str().is_some());
    assert_eq!(
        std::fs::read_to_string(envdump).unwrap().trim(),
        "PostToolUse|echo|src/a.rs"
    );
}

#[tokio::test]
async fn trongo_payload_format_is_opt_in() {
    let out = tempfile::tempdir().unwrap();
    let dump = out.path().join("payload.trongo");
    let mut hook = HookSpec::new(HookEvent::PostToolUse, format!("cat > {}", dump.display()));
    hook.payload_format = PayloadFormat::Trongo;
    let mock = MockProvider::new()
        .respond_with_tool_call("echo", json!({"text": "hi"}))
        .respond("ok");
    run_with_hooks(vec![hook], mock).await;
    let text = std::fs::read_to_string(dump).unwrap();
    let v: HookPayload = cosmos_llm_agent_core::trongo::from_trongo(&text).unwrap();
    assert_eq!(v.event, HookEvent::PostToolUse);
    assert_eq!(v.tool.as_deref(), Some("echo"));
    assert_eq!(v.result, Some(json!("hi")));
}

#[tokio::test]
async fn a_failing_hook_is_a_warning_and_does_not_block() {
    let hook = HookSpec::new(HookEvent::PreToolUse, "echo boom >&2; exit 1");
    let mock = MockProvider::new()
        .respond_with_tool_call("echo", json!({"text": "hi"}))
        .respond("ok");
    let (ev, runs, _) = run_with_hooks(vec![hook], mock).await;
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    assert!(ev.iter().any(
        |e| matches!(e, Event::Warning { message } if message.contains("exited with 1") && message.contains("boom"))
    ));
}

#[tokio::test]
async fn a_slow_hook_times_out_without_blocking() {
    let hook = HookSpec::new(HookEvent::PreToolUse, "sleep 5").timeout_ms(100);
    let mock = MockProvider::new()
        .respond_with_tool_call("echo", json!({"text": "hi"}))
        .respond("ok");
    let (ev, runs, _) = run_with_hooks(vec![hook], mock).await;
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    assert!(ev
        .iter()
        .any(|e| matches!(e, Event::Warning { message } if message.contains("timed out"))));
}

fn count_finished(ev: &[Event]) -> usize {
    ev.iter()
        .filter(|e| matches!(e, Event::RunFinished { .. }))
        .count()
}

#[tokio::test]
async fn stop_hook_block_runs_another_turn_but_is_capped() {
    // A hook that never relents.
    let hook = HookSpec::new(
        HookEvent::Stop,
        r#"cat >/dev/null; echo '{"decision":"block","reason":"tests still failing"}'"#,
    );
    let mut mock = MockProvider::new();
    for i in 0..10 {
        mock = mock.respond(format!("attempt {i}"));
    }
    let (ev, _, rec) = run_with_hooks(vec![hook], mock).await;
    // One original run plus the capped continuations, reported as one input.
    assert_eq!(rec.requests().len(), 4);
    assert_eq!(count_finished(&ev), 1);
    match ev.last().unwrap() {
        Event::RunFinished { reason, steps, .. } => {
            assert_eq!(*reason, Some(StopReason::Finished));
            assert_eq!(*steps, 4);
        }
        _ => unreachable!(),
    }
    let sent = rec.last_request().unwrap();
    assert!(sent
        .messages
        .iter()
        .any(|m| m.text().contains("tests still failing")));
}

#[tokio::test]
async fn stop_hook_that_relents_ends_the_run() {
    let dir = tempfile::tempdir().unwrap();
    let flag = dir.path().join("seen");
    // Blocks the first time, passes the second.
    let hook = HookSpec::new(
        HookEvent::Stop,
        format!(
            r#"cat >/dev/null; if [ -e {f} ]; then exit 0; else touch {f}; echo '{{"decision":"block","reason":"once more"}}'; fi"#,
            f = flag.display()
        ),
    );
    let mock = MockProvider::new()
        .respond("one")
        .respond("two")
        .respond("three");
    let (ev, _, rec) = run_with_hooks(vec![hook], mock).await;
    assert_eq!(rec.requests().len(), 2);
    assert_eq!(count_finished(&ev), 1);
}

#[tokio::test]
async fn user_prompt_submit_can_block_or_add_context() {
    let blocker = HookSpec::new(HookEvent::UserPromptSubmit, "echo 'no secrets' >&2; exit 2");
    let rec_mock = MockProvider::new().respond("never");
    let (ev, _, rec) = run_with_hooks(vec![blocker], rec_mock).await;
    assert!(rec.requests().is_empty(), "the model must not be called");
    assert!(ev
        .iter()
        .any(|e| matches!(e, Event::Warning { message } if message.contains("no secrets"))));
    assert_eq!(count_finished(&ev), 1);

    // Plain stdout on exit 0 is context.
    let adder = HookSpec::new(
        HookEvent::UserPromptSubmit,
        "cat >/dev/null; echo 'branch: main'",
    );
    let (_, _, rec) = run_with_hooks(vec![adder], MockProvider::new().respond("ok")).await;
    let last = rec.last_request().unwrap();
    let user = last
        .messages
        .iter()
        .rev()
        .find(|m| m.role == cosmos_llm::Role::User)
        .unwrap();
    let text = user.text();
    assert!(
        text.contains("go") && text.contains("branch: main"),
        "{text}"
    );
}

#[tokio::test]
async fn session_start_context_rides_with_the_first_input() {
    let hook = HookSpec::new(HookEvent::SessionStart, "echo 'repo uses nix'");
    let (_, _, rec) = run_with_hooks(vec![hook], MockProvider::new().respond("ok")).await;
    let first = rec.requests().remove(0);
    let user = first
        .messages
        .iter()
        .find(|m| m.role == cosmos_llm::Role::User)
        .unwrap();
    assert!(user.text().contains("repo uses nix"));
}

#[tokio::test]
async fn hooks_see_denied_calls_never() {
    // The approver chain runs first: a call the policy denies never reaches a hook.
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let marker = cwd.path().join("hook-ran");
    let (reg, runs) = counting_registry();
    let mut cfg = config(cwd.path(), sess.path());
    cfg.permissions =
        PolicySpec::new(ApprovalMode::FullAuto).deny(cosmos_llm_agent_core::Rule::tool("echo"));
    let hook = HookSpec::new(HookEvent::PreToolUse, format!("touch {}", marker.display()));
    let (h, mut rx) = Core::builder(cfg)
        .completer(completer(
            MockProvider::new()
                .respond_with_tool_call("echo", json!({"text": "x"}))
                .respond("ok"),
        ))
        .tools(reg)
        .hooks(vec![hook])
        .start()
        .unwrap();
    h.user_text("go").await.unwrap();
    until(&mut rx, run_finished).await;
    assert_eq!(runs.load(Ordering::SeqCst), 0);
    assert!(!marker.exists());
}

#[tokio::test]
async fn runner_reports_what_it_ran() {
    let runner = HookRunner::new(
        vec![
            HookSpec::new(
                HookEvent::PreToolUse,
                r#"cat >/dev/null; echo '{"additional_context":"a"}'"#,
            ),
            HookSpec::new(
                HookEvent::PreToolUse,
                r#"cat >/dev/null; echo '{"additional_context":"b"}'"#,
            )
            .matcher("other"),
            HookSpec::new(HookEvent::Stop, "exit 0"),
        ],
        std::env::temp_dir(),
    );
    let mut p = runner.payload(HookEvent::PreToolUse);
    p.tool = Some("echo".into());
    let out = runner.fire(&p, &CancellationToken::new()).await;
    assert_eq!(out.ran, 1);
    assert_eq!(out.context, ["a"]);
    assert!(runner.has(HookEvent::Stop));
    assert!(!runner.has(HookEvent::SessionStart));
}

// --- trust -----------------------------------------------------------------

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn opts(cwd: &Path, user: &Path, project: &Path) -> LoadOptions {
    LoadOptions::new(cwd)
        .user_dir(user)
        .project_dir(project)
        .snapshot(EvalSnapshot::empty())
}

const PROJECT_HOOKS: &str =
    r#"{:hooks {:post-tool-use [{:matcher "edit|write" :command "cargo fmt" :timeout-ms 10000}]}}"#;

#[test]
fn project_hooks_need_trust_by_hash() {
    let (cwd, user, project, store) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    write(
        user.path(),
        "config.trongo",
        r#"{:hooks {:stop [{:command "true"}]}}"#,
    );
    write(project.path(), "config.trongo", PROJECT_HOOKS);
    let trust = HookTrust::new(store.path().join("trusted.json"));
    let o = opts(cwd.path(), user.path(), project.path());

    // Untrusted: the user's hook is active, the project's is pending and warned about.
    let loaded = LoadedHooks::load(&o, Some(&trust));
    assert_eq!(loaded.active.len(), 1);
    assert_eq!(loaded.active[0].event, HookEvent::Stop);
    let pending = loaded.pending.clone().expect("pending project hooks");
    assert_eq!(pending.hooks[0].command, "cargo fmt");
    assert_eq!(pending.hooks[0].timeout_ms, 10000);
    assert!(loaded.warnings.iter().any(|w| w.contains("not trusted")));

    // Approving the hash activates them.
    trust.trust(&pending.project, &pending.hash).unwrap();
    let loaded = LoadedHooks::load(&o, Some(&trust));
    assert_eq!(loaded.active.len(), 2);
    assert!(loaded.pending.is_none());

    // Editing the hook invalidates the approval.
    write(
        project.path(),
        "config.trongo",
        &PROJECT_HOOKS.replace("cargo fmt", "rm -rf /"),
    );
    let loaded = LoadedHooks::load(&o, Some(&trust));
    assert_eq!(loaded.active.len(), 1);
    assert!(loaded.pending.is_some());

    // The blanket flag trusts the project outright.
    let loaded = LoadedHooks::load(&o.clone().trust_project(true), None);
    assert_eq!(loaded.active.len(), 2);
}

#[tokio::test]
async fn untrusted_project_hook_is_not_run_and_a_warning_is_emitted() {
    let (cwd, sess, user, project) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    let marker = cwd.path().join("ran");
    write(
        project.path(),
        "hooks.trongo",
        &format!(
            r#"{{:pre-tool-use [{{:command "touch {}"}}]}}"#,
            marker.display()
        ),
    );
    let (reg, runs) = counting_registry();
    let (h, mut rx) = Core::builder(full_auto(cwd.path(), sess.path()))
        .completer(completer(
            MockProvider::new()
                .respond_with_tool_call("echo", json!({"text": "x"}))
                .respond("ok"),
        ))
        .tools(reg)
        .extensions(&opts(cwd.path(), user.path(), project.path()), None)
        .start()
        .unwrap();
    h.user_text("go").await.unwrap();
    let ev = until(&mut rx, run_finished).await;
    assert_eq!(runs.load(Ordering::SeqCst), 1);
    assert!(!marker.exists(), "the untrusted hook must not run");
    assert!(ev
        .iter()
        .any(|e| matches!(e, Event::Warning { message } if message.contains("not trusted"))));
}
