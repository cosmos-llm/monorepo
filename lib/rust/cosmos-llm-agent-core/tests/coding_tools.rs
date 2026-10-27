//! The core over the real coding tools: policy classification of `bash`,
//! turn diffs, undo, and file changes in the session log.

mod common;

use std::sync::Arc;

use common::*;
use cosmos_llm::providers::mock::MockProvider;
use cosmos_llm_agent_core::session::SessionLog;
use cosmos_llm_agent_core::{Core, Event, Op, Record};
use cosmos_llm_coding_tools::{CodingTools, DiskFs};
use cosmos_llm_tool::{AgentEvent, Decision};
use serde_json::json;

#[tokio::test]
async fn write_asks_then_diffs_then_undoes() {
    let cwd = tempfile::tempdir().unwrap();
    let sess = tempfile::tempdir().unwrap();
    let tools = CodingTools::new(Arc::new(DiskFs::new(cwd.path()).unwrap()));
    let mock = MockProvider::new()
        .respond_with_tool_call("bash", json!({"command": "ls -la | wc -l"}))
        .respond_with_tool_call("write", json!({"file_path": "a.txt", "content": "hi\n"}))
        .respond("written");
    let (h, mut rx) = Core::builder(config(cwd.path(), sess.path()))
        .completer(completer(mock))
        .coding_tools(&tools)
        .start()
        .unwrap();
    let log = match until(&mut rx, |e| matches!(e, Event::SessionConfigured { .. }))
        .await
        .pop()
    {
        Some(Event::SessionConfigured {
            log_path: Some(p), ..
        }) => p,
        other => panic!("{other:?}"),
    };
    h.user_text("go").await.unwrap();

    // The known-safe shell command never reaches the user; the write does.
    let ev = until(&mut rx, |e| matches!(e, Event::ApprovalRequested { .. })).await;
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::Agent(AgentEvent::ToolFinished { result, .. }) if !result.is_error
    )));
    let Some(Event::ApprovalRequested {
        call_id,
        tool,
        preview,
        ..
    }) = ev.last().cloned()
    else {
        unreachable!()
    };
    assert_eq!(tool, "write");
    assert!(preview.is_some());
    h.approve(call_id, Decision::Allow).await.unwrap();

    let ev = until(&mut rx, run_finished).await;
    let diff = ev.iter().find_map(|e| match e {
        Event::TurnDiff { files } => Some(files.clone()),
        _ => None,
    });
    assert_eq!(diff.unwrap().len(), 1);
    assert_eq!(
        std::fs::read_to_string(cwd.path().join("a.txt")).unwrap(),
        "hi\n"
    );

    h.send(Op::Undo { turn: None }).await.unwrap();
    let ev = until(&mut rx, |e| {
        matches!(e, Event::Undone { .. } | Event::Error { .. })
    })
    .await;
    match ev.last().unwrap() {
        Event::Undone {
            reverted, skipped, ..
        } => {
            assert_eq!(reverted, &vec!["a.txt".to_string()]);
            assert!(skipped.is_empty());
        }
        other => panic!("{other:?}"),
    }
    assert!(!cwd.path().join("a.txt").exists());

    h.shutdown().await.unwrap();
    until(&mut rx, |e| matches!(e, Event::ShutdownComplete)).await;

    // The log kept the turn's file changes for undo after a restart.
    let raw = std::fs::read_to_string(&log).unwrap();
    assert!(raw.contains("\"type\":\"file_changes\""));
    let s = SessionLog::load(&log).unwrap();
    assert_eq!(s.file_changes.len(), 1);
    let _ = Record::ModelChanged {
        model: String::new(),
    };
}

#[tokio::test]
async fn dangerous_shell_commands_are_asked_about() {
    let cwd = tempfile::tempdir().unwrap();
    let sess = tempfile::tempdir().unwrap();
    let tools = CodingTools::new(Arc::new(DiskFs::new(cwd.path()).unwrap()));
    let mock = MockProvider::new()
        .respond_with_tool_call("bash", json!({"command": "ls $(echo /)"}))
        .respond("never");
    let (h, mut rx) = Core::builder(config(cwd.path(), sess.path()))
        .completer(completer(mock))
        .coding_tools(&tools)
        .start()
        .unwrap();
    h.user_text("go").await.unwrap();
    let ev = until(&mut rx, |e| matches!(e, Event::ApprovalRequested { .. })).await;
    let Some(Event::ApprovalRequested { call_id, tool, .. }) = ev.last().cloned() else {
        unreachable!()
    };
    assert_eq!(tool, "bash");
    h.approve(call_id, Decision::Abort).await.unwrap();
    let ev = until(&mut rx, run_finished).await;
    assert!(matches!(
        ev.last().unwrap(),
        Event::RunFinished {
            reason: Some(cosmos_llm_tool::StopReason::Aborted),
            ..
        }
    ));
}
