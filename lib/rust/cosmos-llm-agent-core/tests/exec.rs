//! Headless exec (220): output shapes, approvals, limits, interrupt.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::*;
use cosmos_llm::providers::mock::MockProvider;
use cosmos_llm::{Choice, CompletionResponse, ContentBlock, Message, Role, ToolCall, Usage};
use cosmos_llm_agent_core::session::SessionLog;
use cosmos_llm_agent_core::trongo::from_trongo;
use cosmos_llm_agent_core::{
    run_exec, ApprovalMode, Core, CoreConfig, ExecOptions, ExecResult, ExecStop, OutputFormat,
};
use cosmos_llm_coding_tools::{CodingTools, DiskFs};
use cosmos_llm_tool::Registry;
use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

struct Run {
    result: ExecResult,
    stdout: String,
    stderr: String,
    rec: Arc<Recording>,
}

async fn exec(
    mock: MockProvider,
    cfg: CoreConfig,
    registry: Registry,
    opts: ExecOptions,
    token: CancellationToken,
) -> Run {
    let rec = completer(mock);
    let (h, mut rx) = Core::builder(cfg)
        .completer(rec.clone())
        .tools(registry)
        .start()
        .unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let result = run_exec(&h, &mut rx, "go", &opts, &token, &mut out, &mut err)
        .await
        .unwrap();
    Run {
        result,
        stdout: String::from_utf8(out).unwrap(),
        stderr: String::from_utf8(err).unwrap(),
        rec,
    }
}

fn dirs() -> (tempfile::TempDir, tempfile::TempDir) {
    (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap())
}

#[tokio::test]
async fn json_output_has_the_documented_shape() {
    let (cwd, sess) = dirs();
    let r = exec(
        MockProvider::new().respond("all done"),
        config(cwd.path(), sess.path()),
        registry(),
        ExecOptions::new(OutputFormat::Json),
        CancellationToken::new(),
    )
    .await;
    assert_eq!(r.result.exit_code(), 0);
    let v: Value = serde_json::from_str(r.stdout.trim()).unwrap();
    assert_eq!(v["v"], 1);
    assert_eq!(v["result"], "all done");
    assert_eq!(v["stop_reason"], "finished");
    assert_eq!(v["turns"], 1);
    assert!(v["session_id"].as_str().unwrap().len() > 8);
    assert!(v["usage"].is_object());
    assert!(r.stderr.is_empty());
    let back: ExecResult = serde_json::from_value(v).unwrap();
    assert_eq!(back, r.result);
}

#[tokio::test]
async fn jsonl_and_trongo_stream_events_then_the_result() {
    let (cwd, sess) = dirs();
    let r = exec(
        MockProvider::new().respond("hello"),
        config(cwd.path(), sess.path()),
        registry(),
        ExecOptions::new(OutputFormat::Jsonl),
        CancellationToken::new(),
    )
    .await;
    let lines: Vec<Value> = r
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines[0]["event"], "session_configured");
    assert!(lines.iter().any(|l| l["event"] == "run_finished"));
    let last = lines.last().unwrap();
    assert_eq!(last["event"], "exec_result");
    assert_eq!(last["result"], "hello");

    let (cwd, sess) = dirs();
    let r = exec(
        MockProvider::new().respond("hello"),
        config(cwd.path(), sess.path()),
        registry(),
        ExecOptions::new(OutputFormat::Trongo),
        CancellationToken::new(),
    )
    .await;
    assert!(r.stdout.contains(":event \"session_configured\""));
    let tail = &r.stdout[r.stdout.rfind("{\n").unwrap_or(0)..];
    assert!(r.stdout.contains(":event \"exec_result\""), "{tail}");
    // The last literal evaluates back to the result.
    let last = r.stdout.trim_end();
    let start = last.rfind("\n{").map_or(0, |i| i + 1);
    let v: Value = from_trongo(&last[start..]).unwrap();
    assert_eq!(v["result"], "hello");
}

#[tokio::test]
async fn text_output_streams_text_and_summarizes_tools_on_stderr() {
    let (cwd, sess) = dirs();
    let r = exec(
        MockProvider::new()
            .respond_with_tool_call("peek", json!({}))
            .respond("finished peeking"),
        config(cwd.path(), sess.path()),
        registry(),
        ExecOptions::default(),
        CancellationToken::new(),
    )
    .await;
    assert!(r.stdout.contains("finished peeking"));
    assert!(r.stderr.contains("[tool] peek"), "{}", r.stderr);
    assert_eq!(r.result.result, "finished peeking");
}

#[tokio::test]
async fn an_approval_is_denied_with_a_reason_and_the_run_continues() {
    let (cwd, sess) = dirs();
    let tools = CodingTools::new(Arc::new(DiskFs::new(cwd.path()).unwrap()));
    let mut reg = Registry::new();
    tools.register_all(&mut reg);
    let mock = MockProvider::new()
        .respond_with_tool_call("write", json!({"file_path": "a.txt", "content": "hi\n"}))
        .respond("could not write");
    let mut cfg = config(cwd.path(), sess.path());
    cfg.permissions.mode = ApprovalMode::Ask;
    let r = exec(
        mock,
        cfg,
        reg,
        ExecOptions::new(OutputFormat::Json),
        CancellationToken::new(),
    )
    .await;
    assert_eq!(r.result.exit_code(), 0);
    assert_eq!(r.result.denied.len(), 1);
    assert!(!cwd.path().join("a.txt").exists());
    // The model saw the reason.
    let seen = serde_json::to_string(&r.rec.last_request().unwrap().messages).unwrap();
    assert!(seen.contains("non-interactive"), "{seen}");
}

fn tool_call_loop(n: usize) -> MockProvider {
    (0..n).fold(MockProvider::new(), |m, _| {
        m.respond_with_tool_call("peek", json!({}))
    })
}

#[tokio::test]
async fn max_turns_exits_with_two() {
    let (cwd, sess) = dirs();
    let mut cfg = config(cwd.path(), sess.path());
    cfg.max_steps = 1;
    let r = exec(
        tool_call_loop(5),
        cfg,
        registry(),
        ExecOptions::new(OutputFormat::Json),
        CancellationToken::new(),
    )
    .await;
    assert_eq!(r.result.stop_reason, ExecStop::MaxTurns);
    assert_eq!(r.result.exit_code(), 2);
}

fn costly_tool_call(cost: f64) -> CompletionResponse {
    let mut msg = Message::from_blocks(
        Role::Assistant,
        vec![ContentBlock::ToolUse {
            id: format!("c{cost}"),
            name: "peek".into(),
            input: json!({}),
        }],
    );
    msg.name = None;
    CompletionResponse {
        id: None,
        model: None,
        choices: vec![Choice {
            index: 0,
            message: msg,
            finish_reason: Some("tool_calls".into()),
            tool_calls: vec![ToolCall {
                id: format!("c{cost}"),
                name: "peek".into(),
                input: json!({}),
            }],
        }],
        usage: Some(Usage::from_tokens(10, 5).with_cost(cost)),
        route: None,
    }
}

#[tokio::test]
async fn max_cost_exits_with_two() {
    let (cwd, sess) = dirs();
    let mut mock = MockProvider::new();
    for i in 0..300 {
        mock = mock.respond_with(costly_tool_call(0.5 + f64::from(i) * 0.0001));
    }
    let r = exec(
        mock,
        config(cwd.path(), sess.path()),
        registry(),
        ExecOptions::new(OutputFormat::Json).max_cost(0.4),
        CancellationToken::new(),
    )
    .await;
    assert_eq!(r.result.stop_reason, ExecStop::MaxCost, "{:?}", r.result);
    assert_eq!(r.result.exit_code(), 2);
    assert!(r.result.cost.unwrap() >= 0.4);
}

#[tokio::test]
async fn sigint_during_a_slow_tool_exits_130_with_a_valid_session() {
    let (cwd, sess) = dirs();
    let mut cfg = config(cwd.path(), sess.path());
    cfg.permissions.mode = ApprovalMode::FullAuto;
    let token = CancellationToken::new();
    {
        let t = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            t.cancel();
        });
    }
    let r = exec(
        MockProvider::new()
            .respond_with_tool_call("slow", json!({}))
            .respond("never"),
        cfg,
        registry(),
        ExecOptions::new(OutputFormat::Json),
        token,
    )
    .await;
    assert_eq!(r.result.stop_reason, ExecStop::Interrupted);
    assert_eq!(r.result.exit_code(), 130);
    // The log written during the run loads cleanly.
    let mut logs = Vec::new();
    for proj in std::fs::read_dir(sess.path()).unwrap() {
        for f in std::fs::read_dir(proj.unwrap().path()).unwrap() {
            let p = f.unwrap().path();
            if p.extension().is_some_and(|e| e == "jsonl") {
                logs.push(p);
            }
        }
    }
    assert_eq!(logs.len(), 1);
    let s = SessionLog::load(&logs[0]).unwrap();
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    cosmos_llm_tool::validate_history(s.thread.history()).unwrap();
}

#[tokio::test]
async fn a_provider_failure_is_exit_one() {
    let (cwd, sess) = dirs();
    let r = exec(
        MockProvider::new().fail_with(|| cosmos_llm::CosmosError::ContextLength {
            provider: "mock".into(),
            limit: None,
        }),
        {
            let mut c = config(cwd.path(), sess.path());
            c.compaction.enabled = false;
            c
        },
        registry(),
        ExecOptions::new(OutputFormat::Json),
        CancellationToken::new(),
    )
    .await;
    assert_eq!(r.result.exit_code(), 1);
    assert!(r.result.error.is_some());
}
