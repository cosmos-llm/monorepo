//! The protocol is the integration test surface (210): drive `Core` with a
//! mock provider and assert on event sequences.

mod common;

use common::*;
use cosmos_llm::providers::mock::MockProvider;
use cosmos_llm::{CosmosError, Message};
use cosmos_llm_agent_core::session::SessionLog;
use cosmos_llm_agent_core::{
    ApprovalMode, CompactionOutcome, CompactionPlan, Compactor, Core, CoreError, Event, Op,
    Pressure, ResumeFrom, PROTOCOL_VERSION,
};
use cosmos_llm_tool::{AgentEvent, Decision, StopReason, TurnStop};
use futures_util::future::BoxFuture;
use serde_json::json;

fn text_of(events: &[Event]) -> String {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Agent(AgentEvent::TextDelta { text }) => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn user_input_streams_text_and_finishes() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mock = MockProvider::new().respond("Hello there");
    let (h, mut rx) =
        Core::start(config(cwd.path(), sess.path()), completer(mock), registry()).unwrap();

    let first = until(&mut rx, |e| matches!(e, Event::SessionConfigured { .. })).await;
    match first.last().unwrap() {
        Event::SessionConfigured {
            protocol,
            model,
            mode,
            ..
        } => {
            assert_eq!(*protocol, PROTOCOL_VERSION);
            assert_eq!(model, "mock-model");
            assert_eq!(*mode, ApprovalMode::Ask);
        }
        _ => unreachable!(),
    }

    h.user_text("hi").await.unwrap();
    let ev = until(&mut rx, run_finished).await;
    assert_eq!(text_of(&ev), "Hello there");
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::Agent(AgentEvent::TurnFinished {
            stop: TurnStop::EndTurn,
            ..
        })
    )));
    match ev.last().unwrap() {
        Event::RunFinished { reason, steps, .. } => {
            assert_eq!(*reason, Some(StopReason::Finished));
            assert_eq!(*steps, 1);
        }
        _ => unreachable!(),
    }

    h.shutdown().await.unwrap();
    until(&mut rx, |e| matches!(e, Event::ShutdownComplete)).await;
    assert!(rx.recv().await.is_none());
}

#[tokio::test]
async fn tool_call_in_ask_mode_waits_for_approval() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mock = MockProvider::new()
        .respond_with_tool_call("echo", json!({"text": "ping"}))
        .respond("all done");
    let (h, mut rx) =
        Core::start(config(cwd.path(), sess.path()), completer(mock), registry()).unwrap();

    h.user_text("go").await.unwrap();
    let ev = until(&mut rx, |e| matches!(e, Event::ApprovalRequested { .. })).await;
    let (call_id, tool) = match ev.last().unwrap() {
        Event::ApprovalRequested {
            call_id,
            tool,
            input,
            ..
        } => {
            assert_eq!(input, &json!({"text": "ping"}));
            (call_id.clone(), tool.clone())
        }
        _ => unreachable!(),
    };
    assert_eq!(tool, "echo");
    // The request is announced after the call it belongs to.
    assert!(ev
        .iter()
        .any(|e| matches!(e, Event::Agent(AgentEvent::ToolCallReady { .. }))));

    h.approve(call_id.clone(), Decision::Allow).await.unwrap();
    let ev = until(&mut rx, run_finished).await;
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::Agent(AgentEvent::ApprovalResolved {
            decision: Decision::Allow,
            ..
        })
    )));
    let finished = ev
        .iter()
        .find_map(|e| match e {
            Event::Agent(AgentEvent::ToolFinished { result, .. }) => Some(result.clone()),
            _ => None,
        })
        .unwrap();
    assert!(!finished.is_error);
    assert_eq!(text_of(&ev), "all done");
}

#[tokio::test]
async fn denial_reaches_the_model_as_a_tool_error() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mock = MockProvider::new()
        .respond_with_tool_call("echo", json!({"text": "x"}))
        .respond("ok, not doing that");
    let (h, mut rx) =
        Core::start(config(cwd.path(), sess.path()), completer(mock), registry()).unwrap();
    h.user_text("go").await.unwrap();
    let ev = until(&mut rx, |e| matches!(e, Event::ApprovalRequested { .. })).await;
    let Event::ApprovalRequested { call_id, .. } = ev.last().unwrap().clone() else {
        unreachable!()
    };
    h.approve(call_id, Decision::deny("not today"))
        .await
        .unwrap();
    let ev = until(&mut rx, run_finished).await;
    let err = ev
        .iter()
        .find_map(|e| match e {
            Event::Agent(AgentEvent::ToolFinished { result, .. }) => Some(result.as_text()),
            _ => None,
        })
        .unwrap();
    assert!(err.contains("not today"), "{err}");
}

#[tokio::test]
async fn policy_decides_without_bothering_the_user() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mock = MockProvider::new()
        .respond_with_tool_call("peek", json!({}))
        .respond("seen");
    let (h, mut rx) =
        Core::start(config(cwd.path(), sess.path()), completer(mock), registry()).unwrap();
    h.user_text("look").await.unwrap();
    let ev = until(&mut rx, run_finished).await;
    assert!(!ev
        .iter()
        .any(|e| matches!(e, Event::ApprovalRequested { .. })));
    assert!(!ev
        .iter()
        .any(|e| matches!(e, Event::Agent(AgentEvent::ApprovalResolved { .. }))));
    assert!(ev
        .iter()
        .any(|e| matches!(e, Event::Agent(AgentEvent::ToolFinished { .. }))));
}

#[tokio::test]
async fn read_only_mode_denies_with_a_reason() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut cfg = config(cwd.path(), sess.path());
    cfg.permissions = cosmos_llm_agent_core::PolicySpec::new(ApprovalMode::ReadOnly);
    let mock = MockProvider::new()
        .respond_with_tool_call("echo", json!({"text": "x"}))
        .respond("fine");
    let (h, mut rx) = Core::start(cfg, completer(mock), registry()).unwrap();
    h.user_text("go").await.unwrap();
    let ev = until(&mut rx, run_finished).await;
    assert!(!ev
        .iter()
        .any(|e| matches!(e, Event::ApprovalRequested { .. })));
    let out = ev
        .iter()
        .find_map(|e| match e {
            Event::Agent(AgentEvent::ToolFinished { result, .. }) => Some(result.as_text()),
            _ => None,
        })
        .unwrap();
    assert!(out.contains("read-only"), "{out}");
}

#[tokio::test]
async fn set_mode_takes_effect_on_the_next_call() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mock = MockProvider::new()
        .respond_with_tool_call("echo", json!({"text": "x"}))
        .respond("done");
    let (h, mut rx) =
        Core::start(config(cwd.path(), sess.path()), completer(mock), registry()).unwrap();
    h.send(Op::SetMode {
        mode: ApprovalMode::FullAuto,
    })
    .await
    .unwrap();
    until(&mut rx, |e| {
        matches!(
            e,
            Event::ModeChanged {
                mode: ApprovalMode::FullAuto
            }
        )
    })
    .await;
    h.user_text("go").await.unwrap();
    let ev = until(&mut rx, run_finished).await;
    assert!(!ev
        .iter()
        .any(|e| matches!(e, Event::ApprovalRequested { .. })));
}

#[tokio::test]
async fn interrupt_during_a_slow_tool_ends_the_turn() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut cfg = config(cwd.path(), sess.path());
    cfg.permissions = cosmos_llm_agent_core::PolicySpec::new(ApprovalMode::FullAuto);
    let mock = MockProvider::new().respond_with_tool_call("slow", json!({}));
    let (h, mut rx) = Core::start(cfg, completer(mock), registry()).unwrap();
    h.user_text("go").await.unwrap();
    until(&mut rx, |e| {
        matches!(e, Event::Agent(AgentEvent::ToolStarted { .. }))
    })
    .await;
    let t = std::time::Instant::now();
    h.interrupt().await.unwrap();
    let ev = until(&mut rx, run_finished).await;
    assert!(t.elapsed() < std::time::Duration::from_secs(5));
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::Agent(AgentEvent::TurnFinished {
            stop: TurnStop::Interrupted,
            ..
        })
    )));
    match ev.last().unwrap() {
        Event::RunFinished { reason, .. } => assert_eq!(*reason, Some(StopReason::Interrupted)),
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn shutdown_during_a_turn_completes() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut cfg = config(cwd.path(), sess.path());
    cfg.permissions = cosmos_llm_agent_core::PolicySpec::new(ApprovalMode::FullAuto);
    let mock = MockProvider::new().respond_with_tool_call("slow", json!({}));
    let (h, mut rx) = Core::start(cfg, completer(mock), registry()).unwrap();
    h.user_text("go").await.unwrap();
    until(&mut rx, |e| {
        matches!(e, Event::Agent(AgentEvent::ToolStarted { .. }))
    })
    .await;
    h.shutdown().await.unwrap();
    until(&mut rx, |e| matches!(e, Event::ShutdownComplete)).await;
    assert!(h.user_text("late").await.is_err() || rx.recv().await.is_none());
}

#[tokio::test]
async fn the_session_is_logged_and_can_be_resumed() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mock = MockProvider::new()
        .respond_with_tool_call("peek", json!({}))
        .respond("first answer");
    let (h, mut rx) =
        Core::start(config(cwd.path(), sess.path()), completer(mock), registry()).unwrap();
    let log = match until(&mut rx, |e| matches!(e, Event::SessionConfigured { .. }))
        .await
        .pop()
    {
        Some(Event::SessionConfigured {
            log_path: Some(p), ..
        }) => p,
        other => panic!("{other:?}"),
    };
    h.user_text("question").await.unwrap();
    until(&mut rx, run_finished).await;
    h.shutdown().await.unwrap();
    until(&mut rx, |e| matches!(e, Event::ShutdownComplete)).await;

    let s = SessionLog::load(&log).unwrap();
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    // user, assistant(tool call), tool result, assistant
    assert_eq!(s.thread.history().len(), 4);
    assert_eq!(s.msg_ids, vec![1, 2, 3, 4]);

    // Continue: the transcript is replayed and the thread carries on.
    let rec = completer(MockProvider::new().respond("second answer"));
    let (h, mut rx) = Core::builder(config(cwd.path(), sess.path()))
        .completer(rec.clone())
        .tools(registry())
        .resume(ResumeFrom::Latest)
        .start()
        .unwrap();
    let ev = until(&mut rx, |e| matches!(e, Event::HistoryLoaded { .. })).await;
    match ev.last().unwrap() {
        Event::HistoryLoaded { items } => assert!(items.len() >= 4),
        _ => unreachable!(),
    }
    h.user_text("follow-up").await.unwrap();
    until(&mut rx, run_finished).await;
    // The model saw the whole earlier conversation.
    assert_eq!(rec.last_request().unwrap().messages.len() - 1, 5); // + system prompt
    h.shutdown().await.unwrap();
    until(&mut rx, |e| matches!(e, Event::ShutdownComplete)).await;
    let s = SessionLog::load(&log).unwrap();
    assert_eq!(s.thread.history().len(), 6);
}

#[tokio::test]
async fn set_model_is_applied_and_logged() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let rec = completer(MockProvider::new().respond("ok"));
    let (h, mut rx) =
        Core::start(config(cwd.path(), sess.path()), rec.clone(), registry()).unwrap();
    h.send(Op::SetModel {
        model: "other-model".into(),
    })
    .await
    .unwrap();
    until(&mut rx, |e| matches!(e, Event::ModelChanged { .. })).await;
    h.user_text("hi").await.unwrap();
    until(&mut rx, run_finished).await;
    assert_eq!(rec.last_request().unwrap().model, "other-model");
    h.shutdown().await.unwrap();
    until(&mut rx, |e| matches!(e, Event::ShutdownComplete)).await;
    let latest = cosmos_llm_agent_core::SessionStore::new(sess.path())
        .latest(cwd.path())
        .unwrap()
        .unwrap();
    assert_eq!(latest.model, "other-model");
}

#[tokio::test]
async fn oversized_input_fails_the_turn_with_a_clear_message() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut cfg = config(cwd.path(), sess.path());
    cfg.context.window = Some(200);
    cfg.context.reserve_output = Some(20);
    let rec = completer(MockProvider::new().respond("never"));
    let (h, mut rx) = Core::start(cfg, rec.clone(), registry()).unwrap();
    h.user_text("x".repeat(4_000)).await.unwrap();
    let ev = until(&mut rx, run_finished).await;
    assert!(ev.iter().any(|e| matches!(
        e,
        Event::ContextUsage {
            pressure: Pressure::Overflow,
            ..
        }
    )));
    assert!(ev.iter().any(|e| matches!(e, Event::Error { fatal: false, message } if message.contains("context window"))));
    assert!(rec.requests().is_empty());
}

#[tokio::test]
async fn context_usage_is_reported_after_each_response() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut cfg = config(cwd.path(), sess.path());
    cfg.context.window = Some(100_000);
    let (h, mut rx) = Core::start(
        cfg,
        completer(MockProvider::new().respond("hello")),
        registry(),
    )
    .unwrap();
    h.user_text("hi").await.unwrap();
    let ev = until(&mut rx, run_finished).await;
    let usages: Vec<u32> = ev
        .iter()
        .filter_map(|e| match e {
            Event::ContextUsage { used, window, .. } => {
                assert_eq!(*window, 100_000);
                Some(*used)
            }
            _ => None,
        })
        .collect();
    // One estimate before the call, one from the response.
    assert!(usages.len() >= 2, "{usages:?}");
}

struct Stub;
impl Compactor for Stub {
    fn compact<'a>(
        &'a self,
        _plan: &'a CompactionPlan,
        history: &'a [Message],
    ) -> BoxFuture<'a, Result<CompactionOutcome, CoreError>> {
        Box::pin(async move {
            Ok(CompactionOutcome::new(
                0..history.len(),
                Message::user("[summary]"),
            ))
        })
    }
}

#[tokio::test]
async fn manual_compaction_goes_through_the_compactor_and_the_log() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let rec = completer(MockProvider::new().respond("one").respond("two"));
    let (h, mut rx) = Core::builder(config(cwd.path(), sess.path()))
        .completer(rec.clone())
        .tools(registry())
        .compactor(std::sync::Arc::new(Stub))
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
    h.user_text("hi").await.unwrap();
    until(&mut rx, run_finished).await;
    h.send(Op::Compact { instructions: None }).await.unwrap();
    until(&mut rx, |e| matches!(e, Event::CompactionFinished { .. })).await;
    h.user_text("more").await.unwrap();
    until(&mut rx, run_finished).await;
    // After compaction the model saw: system, summary, new input.
    assert_eq!(rec.last_request().unwrap().messages.len(), 3);
    h.shutdown().await.unwrap();
    until(&mut rx, |e| matches!(e, Event::ShutdownComplete)).await;
    let s = SessionLog::load(&log).unwrap();
    assert_eq!(s.thread.history().len(), 3); // summary, more, two
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
}

#[tokio::test]
async fn a_provider_context_error_compacts_once_and_retries() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mock = MockProvider::new()
        .respond("first")
        .fail_with(|| CosmosError::ContextLength {
            provider: "mock".into(),
            limit: None,
        })
        .respond("after compaction");
    let (h, mut rx) = Core::builder(config(cwd.path(), sess.path()))
        .completer(completer(mock))
        .tools(registry())
        .compactor(std::sync::Arc::new(Stub))
        .start()
        .unwrap();
    h.user_text("one").await.unwrap();
    until(&mut rx, run_finished).await;
    h.user_text("two").await.unwrap();
    let ev = until(&mut rx, run_finished).await;
    assert!(ev
        .iter()
        .any(|e| matches!(e, Event::CompactionFinished { .. })));
    assert_eq!(text_of(&ev), "after compaction");
}
