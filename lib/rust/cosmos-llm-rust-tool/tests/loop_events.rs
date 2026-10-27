//! The streaming loop, threads, approvals and interrupts, against the mock
//! provider and small hand-written completers.
#![cfg(feature = "agent")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use cosmos_llm::providers::mock::{Chunking, MockProvider};
use cosmos_llm::providers::Provider;
use cosmos_llm::{
    CancellationToken, CompletionRequest, CompletionResponse, CompletionStream, ContentBlock,
    CosmosError, Message, Role, StreamChunk, ToolCallDelta,
};
use cosmos_llm_tool::{
    event_channel, validate_history, AgentEvent, AgentLoop, AllowAll, ApprovalScope, Completer,
    Decision, FnApprover, HistoryError, Registry, RunHandles, StopReason, Thread, ThreadStatus,
    ToolDefinition, ToolHints, TurnStop,
};
use futures_util::future::BoxFuture;
use futures_util::stream::{self, StreamExt};
use serde_json::{json, Value};

/// Drives a [`MockProvider`] through both entry points of [`Completer`].
struct Spy(MockProvider);

impl Completer for Spy {
    fn completion<'a>(
        &'a self,
        req: &'a CompletionRequest,
    ) -> BoxFuture<'a, Result<CompletionResponse, CosmosError>> {
        Box::pin(async move { Provider::completion(&self.0, req).await })
    }

    fn stream<'a>(
        &'a self,
        req: &'a CompletionRequest,
    ) -> BoxFuture<'a, Result<CompletionStream, CosmosError>> {
        Box::pin(async move { Provider::stream_completion(&self.0, req).await })
    }
}

/// Only implements `completion`, like an older test double.
struct CompletionOnly(MockProvider);

impl Completer for CompletionOnly {
    fn completion<'a>(
        &'a self,
        req: &'a CompletionRequest,
    ) -> BoxFuture<'a, Result<CompletionResponse, CosmosError>> {
        Box::pin(async move { Provider::completion(&self.0, req).await })
    }
}

/// Streams some chunks, then never finishes, so a test can interrupt it.
struct Hang(Vec<StreamChunk>);

impl Completer for Hang {
    fn completion<'a>(
        &'a self,
        _req: &'a CompletionRequest,
    ) -> BoxFuture<'a, Result<CompletionResponse, CosmosError>> {
        Box::pin(async { std::future::pending().await })
    }

    fn stream<'a>(
        &'a self,
        _req: &'a CompletionRequest,
    ) -> BoxFuture<'a, Result<CompletionStream, CosmosError>> {
        let chunks = self.0.clone();
        Box::pin(async move {
            Ok(
                Box::pin(stream::iter(chunks.into_iter().map(Ok)).chain(stream::pending()))
                    as CompletionStream,
            )
        })
    }
}

fn tool_use(id: &str, name: &str, input: Value) -> ContentBlock {
    ContentBlock::ToolUse {
        id: id.into(),
        name: name.into(),
        input,
    }
}

fn echo_registry() -> Registry {
    let mut registry = Registry::new();
    registry.register(
        ToolDefinition::new("echo")
            .param(
                "value",
                cosmos_llm_tool::ParameterType::String,
                false,
                "text",
            )
            .handler(|p| {
                Ok(json!(format!(
                    "echoed:{}",
                    p["value"].as_str().unwrap_or("")
                )))
            }),
    );
    registry
}

async fn run_collect(
    agent: &AgentLoop,
    client: &dyn Completer,
    history: &mut Vec<Message>,
    cancel: &CancellationToken,
) -> (cosmos_llm_tool::RunOutcome, Vec<AgentEvent>) {
    let (tx, mut rx) = event_channel(4096);
    let outcome = agent.run_on(client, history, &tx, cancel).await.unwrap();
    drop(tx);
    let mut events = Vec::new();
    while let Some(e) = rx.recv().await {
        events.push(e);
    }
    (outcome, events)
}

fn index_of(events: &[AgentEvent], pred: impl Fn(&AgentEvent) -> bool) -> usize {
    events.iter().position(pred).expect("event present")
}

fn tool_results(history: &[Message]) -> Vec<(String, String, bool)> {
    history
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|b| match b {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => Some((
                tool_use_id.clone(),
                content
                    .iter()
                    .filter_map(|c| match c {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect(),
                *is_error,
            )),
            _ => None,
        })
        .collect()
}

// ---- 060: streaming events ------------------------------------------------

#[tokio::test]
async fn a_text_turn_emits_events_in_order_and_deltas_concatenate() {
    let spy = Spy(MockProvider::new()
        .with_chunking(Chunking::PerWord)
        .respond("one two three"));
    let agent = AgentLoop::new(echo_registry()).with_model("m");
    let mut history = vec![Message::user("hi")];

    let (outcome, events) =
        run_collect(&agent, &spy, &mut history, &CancellationToken::new()).await;

    assert_eq!(outcome.text, "one two three");
    assert!(matches!(events[0], AgentEvent::TurnStarted { turn: 1 }));
    let deltas: String = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::TextDelta { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas, "one two three");
    assert!(
        events
            .iter()
            .filter(|e| matches!(e, AgentEvent::TextDelta { .. }))
            .count()
            > 1
    );
    let usage = index_of(&events, |e| matches!(e, AgentEvent::Usage { .. }));
    let finished = index_of(&events, |e| {
        matches!(
            e,
            AgentEvent::TurnFinished {
                stop: TurnStop::EndTurn,
                ..
            }
        )
    });
    assert!(usage < finished);
    assert_eq!(finished, events.len() - 1);
    // The final answer is part of the history now.
    assert_eq!(history.last().unwrap().text(), "one two three");
}

#[tokio::test]
async fn two_parallel_tool_calls_stream_started_before_ready() {
    let spy = Spy(MockProvider::new()
        .with_chunking(Chunking::Fixed(3))
        .respond_with_blocks(vec![
            tool_use("a", "echo", json!({"value": "first"})),
            tool_use("b", "echo", json!({"value": "second"})),
        ])
        .respond("done"));
    let agent = AgentLoop::new(echo_registry()).with_model("m");
    let mut history = vec![Message::user("go")];

    let (_, events) = run_collect(&agent, &spy, &mut history, &CancellationToken::new()).await;

    for (id, value) in [("a", "first"), ("b", "second")] {
        let started = index_of(
            &events,
            |e| matches!(e, AgentEvent::ToolCallStarted { call_id, .. } if call_id == id),
        );
        let ready = index_of(
            &events,
            |e| matches!(e, AgentEvent::ToolCallReady { call_id, .. } if call_id == id),
        );
        assert!(started < ready);
        match &events[ready] {
            AgentEvent::ToolCallReady { input, .. } => assert_eq!(input["value"], value),
            _ => unreachable!(),
        }
    }
    let args: String = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ToolCallArgsDelta { call_id, delta } if call_id == "a" => {
                Some(delta.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        serde_json::from_str::<Value>(&args).unwrap()["value"],
        "first"
    );
    assert_eq!(
        tool_results(&history),
        vec![
            ("a".into(), "echoed:first".into(), false),
            ("b".into(), "echoed:second".into(), false)
        ]
    );
}

#[tokio::test]
async fn a_completion_only_completer_yields_one_big_delta() {
    let client = CompletionOnly(MockProvider::new().respond("the whole answer"));
    let agent = AgentLoop::new(echo_registry()).with_model("m");
    let mut history = vec![Message::user("hi")];

    let (_, events) = run_collect(&agent, &client, &mut history, &CancellationToken::new()).await;

    let deltas: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::TextDelta { text } => Some(text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(deltas, vec!["the whole answer".to_owned()]);
}

#[tokio::test]
async fn a_slow_consumer_on_a_tiny_channel_loses_nothing() {
    let script = || {
        Spy(MockProvider::new()
            .with_chunking(Chunking::PerCharacter)
            .respond_with_blocks(vec![tool_use("a", "echo", json!({"value": "x"}))])
            .respond("finished it"))
    };
    let agent = AgentLoop::new(echo_registry()).with_model("m");

    let mut reference_history = vec![Message::user("go")];
    let (_, reference) = run_collect(
        &agent,
        &script(),
        &mut reference_history,
        &CancellationToken::new(),
    )
    .await;

    let (tx, mut rx) = event_channel(1);
    let consumer = tokio::spawn(async move {
        let mut got = Vec::new();
        while let Some(e) = rx.recv().await {
            tokio::time::sleep(Duration::from_micros(200)).await;
            got.push(e);
        }
        got
    });
    let mut history = vec![Message::user("go")];
    agent
        .run_on(&script(), &mut history, &tx, &CancellationToken::new())
        .await
        .unwrap();
    drop(tx);

    assert_eq!(consumer.await.unwrap(), reference);
}

#[tokio::test]
async fn thinking_streams_as_events_and_stays_in_history() {
    let spy = Spy(MockProvider::new().respond_with_blocks(vec![
        ContentBlock::Thinking {
            text: "hmm".into(),
            signature: Some("sig".into()),
        },
        ContentBlock::text("answer"),
    ]));
    let agent = AgentLoop::new(echo_registry()).with_model("m");
    let mut history = vec![Message::user("hi")];

    let (_, events) = run_collect(&agent, &spy, &mut history, &CancellationToken::new()).await;

    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::ThinkingDelta { text } if text == "hmm")));
    assert!(matches!(
        &history.last().unwrap().content[0],
        ContentBlock::Thinking { signature: Some(s), .. } if s == "sig"
    ));
}

#[tokio::test]
async fn each_request_carries_cache_breakpoints() {
    let spy = Spy(MockProvider::new().respond("ok"));
    let agent = AgentLoop::new(echo_registry())
        .with_model("m")
        .with_system("sys");
    let mut history = vec![Message::user("hi")];

    run_collect(&agent, &spy, &mut history, &CancellationToken::new()).await;

    let req = spy.0.last_request().unwrap();
    assert!(req.cache_tools.is_some());
    assert_eq!(req.messages[0].role, Role::System);
    assert!(req.messages[0].cache.is_some());
    // The system prompt is added per request, not stored in the history.
    assert_eq!(history[0].role, Role::User);
}

#[tokio::test]
async fn progress_and_display_reach_the_events() {
    let mut registry = Registry::new();
    registry.register(ToolDefinition::new("build").context_handler(|_, ctx| {
        Box::pin(async move {
            ctx.progress.send("line 1").await;
            ctx.progress.send("line 2").await;
            ctx.set_display(json!({"exit_code": 0}));
            Ok(json!("built"))
        })
    }));
    let spy = Spy(MockProvider::new()
        .respond_with_blocks(vec![tool_use("c", "build", json!({}))])
        .respond("ok"));
    let agent = AgentLoop::new(registry).with_model("m");
    let mut history = vec![Message::user("go")];

    let (_, events) = run_collect(&agent, &spy, &mut history, &CancellationToken::new()).await;

    let chunks: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::ToolOutputDelta { chunk, .. } => Some(chunk.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(chunks, ["line 1", "line 2"]);
    let finished = events
        .iter()
        .find_map(|e| match e {
            AgentEvent::ToolFinished { result, .. } => Some(result),
            _ => None,
        })
        .unwrap();
    assert_eq!(finished.as_text(), "built");
    assert_eq!(finished.display, Some(json!({"exit_code": 0})));
    // The model sees the result, never the display.
    assert_eq!(tool_results(&history)[0].1, "built");
}

#[tokio::test]
async fn oversized_results_are_limited_before_entering_history() {
    let mut registry = Registry::new();
    registry.register(ToolDefinition::new("big").handler(|_| Ok(json!("line\n".repeat(5000)))));
    let spy = Spy(MockProvider::new()
        .respond_with_blocks(vec![tool_use("c", "big", json!({}))])
        .respond("ok"));
    let agent = AgentLoop::new(registry)
        .with_model("m")
        .with_output_limit(cosmos_llm_tool::OutputLimit::new().max_lines(10));
    let mut history = vec![Message::user("go")];

    let (_, events) = run_collect(&agent, &spy, &mut history, &CancellationToken::new()).await;

    let text = &tool_results(&history)[0].1;
    assert!(text.lines().count() <= 10, "{text}");
    assert!(events.iter().any(|e| matches!(
        e,
        AgentEvent::ToolFinished { result, .. } if result.truncated
    )));
}

// ---- 070: threads ---------------------------------------------------------

#[tokio::test]
async fn two_inputs_on_an_idle_thread_share_one_history() {
    let spy = Spy(MockProvider::new()
        .respond("first reply")
        .respond("second reply"));
    let agent = AgentLoop::new(echo_registry()).with_model("m");
    let mut thread = Thread::new("t");
    let handles = RunHandles::new();

    thread.send(&agent, &spy, "one", &handles).await.unwrap();
    let outcome = thread.send(&agent, &spy, "two", &handles).await.unwrap();

    let roles: Vec<_> = thread.history().iter().map(|m| m.role).collect();
    assert_eq!(
        roles,
        [Role::User, Role::Assistant, Role::User, Role::Assistant]
    );
    assert_eq!(outcome.text, "second reply");
    assert_eq!(thread.usage().steps, 2);
    assert_eq!(thread.status(), ThreadStatus::Idle);
    // The second request carried the whole first exchange.
    assert_eq!(spy.0.requests()[1].messages.len(), 3);
}

#[tokio::test]
async fn input_queued_during_a_tool_step_reaches_the_next_model_call() {
    let mut thread = Thread::new("t");
    let queue = thread.queue();

    let mut registry = Registry::new();
    registry.register(ToolDefinition::new("slow_step").handler(move |_| {
        queue.push("also check the docs");
        Ok(json!("stepped"))
    }));
    let spy = Spy(MockProvider::new()
        .respond_with_blocks(vec![tool_use("c", "slow_step", json!({}))])
        .respond("ok, including docs"));
    let agent = AgentLoop::new(registry).with_model("m");
    let (tx, mut rx) = event_channel(1024);

    thread
        .send(&agent, &spy, "start", &RunHandles::new().events(tx))
        .await
        .unwrap();

    let second = &spy.0.requests()[1];
    let last = second.messages.last().unwrap();
    assert_eq!(last.role, Role::User);
    assert_eq!(last.text(), "also check the docs");
    assert!(second.messages[second.messages.len() - 2].is_tool_result());
    let mut delivered = false;
    while let Ok(e) = rx.try_recv() {
        delivered |= matches!(e, AgentEvent::InputDelivered { .. });
    }
    assert!(delivered);
    assert!(thread.queue().is_empty());
}

#[tokio::test]
async fn input_queued_before_send_is_merged_into_one_user_message() {
    let mut thread = Thread::new("t");
    let queue = thread.queue();
    // Queued before the run starts, behind the input `send` pushes.
    queue.push("and this");
    let spy = Spy(MockProvider::new().respond("a").respond("b"));
    let agent = AgentLoop::new(echo_registry()).with_model("m");

    let outcome = thread
        .send(&agent, &spy, "first", &RunHandles::new())
        .await
        .unwrap();

    // Both inputs were merged into one user message: no consecutive users.
    assert_eq!(thread.history()[0].text(), "and this\nfirst");
    assert_eq!(outcome.text, "a");
}

#[test]
fn validate_history_rejects_a_dangling_tool_use_and_accepts_parallel_results() {
    let calls = Message::assistant_blocks(vec![
        tool_use("a", "echo", json!({})),
        tool_use("b", "echo", json!({})),
    ]);
    let dangling = vec![Message::user("hi"), calls.clone()];
    assert!(matches!(
        validate_history(&dangling),
        Err(HistoryError::DanglingToolUse { index: 1, .. })
    ));

    let half = vec![
        Message::user("hi"),
        calls.clone(),
        Message::tool_result("a", "ok"),
        Message::user("next"),
    ];
    assert!(matches!(
        validate_history(&half),
        Err(HistoryError::DanglingToolUse { .. })
    ));

    let ok = vec![
        Message::user("hi"),
        calls,
        Message::tool_result("b", "ok"),
        Message::tool_result("a", "ok"),
        Message::assistant("done"),
    ];
    assert!(validate_history(&ok).is_ok());

    let orphan = vec![Message::user("hi"), Message::tool_result("zzz", "ok")];
    assert!(matches!(
        validate_history(&orphan),
        Err(HistoryError::OrphanToolResult { .. })
    ));
}

#[test]
fn truncate_to_drops_a_user_message_and_what_follows() {
    let mut thread = Thread::with_history(
        "t",
        vec![
            Message::user("one"),
            Message::assistant("reply"),
            Message::user("two"),
            Message::assistant("reply 2"),
        ],
    );
    assert!(thread.truncate_to(1).is_err());
    let dropped = thread.truncate_to(2).unwrap();
    assert_eq!(dropped.text(), "two");
    assert_eq!(thread.history().len(), 2);
}

// ---- 080: approvals -------------------------------------------------------

fn counting_registry(counter: Arc<std::sync::atomic::AtomicUsize>) -> Registry {
    let mut registry = Registry::new();
    registry.register(
        ToolDefinition::new("shell")
            .destructive(true)
            .hints(|input| ToolHints::new().paths(input["path"].as_str()))
            .preview(|input| Some(json!({"will_run": input["cmd"]})))
            .handler(move |p| {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(json!(format!("ran {}", p["cmd"].as_str().unwrap_or(""))))
            })
            .param("cmd", cosmos_llm_tool::ParameterType::String, false, "")
            .param("path", cosmos_llm_tool::ParameterType::String, false, ""),
    );
    registry
}

#[tokio::test]
async fn deny_becomes_an_error_result_and_the_run_continues() {
    let ran = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let spy = Spy(MockProvider::new()
        .respond_with_blocks(vec![tool_use("c", "shell", json!({"cmd": "rm -rf /"}))])
        .respond("understood"));
    let agent = AgentLoop::new(counting_registry(ran.clone()))
        .with_model("m")
        .with_approver(Arc::new(FnApprover::new(|req| {
            assert!(req.hints.destructive);
            assert_eq!(req.preview, Some(json!({"will_run": "rm -rf /"})));
            Decision::deny("too risky")
        })));
    let mut history = vec![Message::user("go")];

    let (outcome, events) =
        run_collect(&agent, &spy, &mut history, &CancellationToken::new()).await;

    assert_eq!(ran.load(Ordering::SeqCst), 0);
    assert_eq!(
        tool_results(&history),
        vec![("c".into(), "User denied: too risky".into(), true)]
    );
    assert_eq!(outcome.text, "understood");
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::ApprovalRequested { .. })));
    assert!(events.iter().any(|e| matches!(
        e,
        AgentEvent::ApprovalResolved {
            decision: Decision::Deny { .. },
            ..
        }
    )));
}

#[tokio::test]
async fn abort_skips_every_remaining_call_but_keeps_the_history_valid() {
    let ran = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let spy = Spy(MockProvider::new().respond_with_blocks(vec![
        tool_use("a", "shell", json!({"cmd": "1"})),
        tool_use("b", "shell", json!({"cmd": "2"})),
        tool_use("c", "shell", json!({"cmd": "3"})),
    ]));
    let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let asked2 = asked.clone();
    let agent = AgentLoop::new(counting_registry(ran.clone()))
        .with_model("m")
        .with_approver(Arc::new(FnApprover::new(move |_| {
            asked2.fetch_add(1, Ordering::SeqCst);
            Decision::Abort
        })));
    let mut history = vec![Message::user("go")];

    let (outcome, events) =
        run_collect(&agent, &spy, &mut history, &CancellationToken::new()).await;

    assert_eq!(ran.load(Ordering::SeqCst), 0);
    assert_eq!(asked.load(Ordering::SeqCst), 1, "no prompt after the abort");
    assert_eq!(outcome.reason, StopReason::Aborted);
    assert_eq!(tool_results(&history).len(), 3);
    assert!(tool_results(&history).iter().all(|(_, _, err)| *err));
    assert!(validate_history(&history).is_ok());
    assert!(matches!(
        events.last(),
        Some(AgentEvent::TurnFinished {
            stop: TurnStop::Aborted,
            ..
        })
    ));
}

#[tokio::test]
async fn allow_always_skips_the_prompt_for_later_calls() {
    let ran = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let spy = Spy(MockProvider::new()
        .respond_with_blocks(vec![tool_use("a", "shell", json!({"cmd": "1"}))])
        .respond_with_blocks(vec![tool_use("b", "shell", json!({"cmd": "2"}))])
        .respond("done"));
    let asked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let asked2 = asked.clone();
    let agent = AgentLoop::new(counting_registry(ran.clone()))
        .with_model("m")
        .with_approver(Arc::new(FnApprover::new(move |_| {
            asked2.fetch_add(1, Ordering::SeqCst);
            Decision::AllowAlways {
                scope: ApprovalScope::Tool {
                    name: "shell".into(),
                },
            }
        })));
    let mut history = vec![Message::user("go")];

    run_collect(&agent, &spy, &mut history, &CancellationToken::new()).await;

    assert_eq!(asked.load(Ordering::SeqCst), 1);
    assert_eq!(ran.load(Ordering::SeqCst), 2);
    assert_eq!(agent.grants().len(), 1);
}

#[tokio::test]
async fn path_scoped_grants_only_cover_paths_under_the_prefix() {
    let scope = ApprovalScope::Path {
        name: "shell".into(),
        prefix: "src".into(),
    };
    let inside = ToolHints::new().paths(["src/a.rs"]);
    let outside = ToolHints::new().paths(["src/a.rs", "etc/passwd"]);
    assert!(scope.covers("shell", &json!({}), &inside));
    assert!(!scope.covers("shell", &json!({}), &outside));
    assert!(!scope.covers("shell", &json!({}), &ToolHints::new()));
}

#[tokio::test]
async fn an_approver_that_allows_everything_changes_nothing() {
    let ran = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let spy = Spy(MockProvider::new()
        .respond_with_blocks(vec![tool_use("a", "shell", json!({"cmd": "1"}))])
        .respond("done"));
    let agent = AgentLoop::new(counting_registry(ran.clone()))
        .with_model("m")
        .with_approver(Arc::new(AllowAll));
    let mut history = vec![Message::user("go")];

    run_collect(&agent, &spy, &mut history, &CancellationToken::new()).await;

    assert_eq!(ran.load(Ordering::SeqCst), 1);
}

// ---- 090: interrupts ------------------------------------------------------

async fn cancel_after(token: CancellationToken, ms: u64) {
    tokio::time::sleep(Duration::from_millis(ms)).await;
    token.cancel();
}

#[tokio::test]
async fn cancel_during_text_streaming_keeps_the_partial_text() {
    let hang = Hang(vec![StreamChunk::text("partial answer")]);
    let agent = AgentLoop::new(echo_registry()).with_model("m");
    let cancel = CancellationToken::new();
    tokio::spawn(cancel_after(cancel.clone(), 50));
    let mut history = vec![Message::user("hi")];

    let (outcome, events) = run_collect(&agent, &hang, &mut history, &cancel).await;

    assert_eq!(outcome.reason, StopReason::Interrupted);
    assert_eq!(history[1].role, Role::Assistant);
    assert_eq!(history[1].text(), "partial answer");
    assert_eq!(history[2].text(), "[Request interrupted by user]");
    assert!(validate_history(&history).is_ok());
    assert!(matches!(
        events.last(),
        Some(AgentEvent::TurnFinished {
            stop: TurnStop::Interrupted,
            ..
        })
    ));

    // The next run on the same history works.
    let next = Spy(MockProvider::new().respond("back"));
    history.push(Message::user("continue"));
    let (outcome, _) = run_collect(&agent, &next, &mut history, &CancellationToken::new()).await;
    assert_eq!(outcome.text, "back");
}

#[tokio::test]
async fn cancel_mid_tool_arguments_leaves_no_tool_use() {
    let hang = Hang(vec![
        StreamChunk::text("let me write that"),
        StreamChunk {
            tool_calls: vec![ToolCallDelta {
                index: 0,
                id: Some("c".into()),
                name: Some("echo".into()),
                arguments: Some(r#"{"value": "unfinis"#.into()),
            }],
            ..Default::default()
        },
    ]);
    let agent = AgentLoop::new(echo_registry())
        .with_model("m")
        .with_interrupt_note(false);
    let cancel = CancellationToken::new();
    tokio::spawn(cancel_after(cancel.clone(), 50));
    let mut history = vec![Message::user("hi")];

    let (_, events) = run_collect(&agent, &hang, &mut history, &cancel).await;

    assert_eq!(history.len(), 2);
    assert!(history[1].tool_calls().next().is_none());
    assert_eq!(history[1].text(), "let me write that");
    // The UI did see the call start, and can discard it.
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::ToolCallStarted { .. })));
    assert!(!events
        .iter()
        .any(|e| matches!(e, AgentEvent::ToolCallReady { .. })));
}

#[tokio::test]
async fn cancel_with_one_tool_done_and_one_sleeping() {
    let saw_cancel = Arc::new(AtomicBool::new(false));
    let saw = saw_cancel.clone();
    let mut registry = echo_registry();
    registry.register(
        ToolDefinition::new("sleepy").context_handler(move |_, ctx| {
            let saw = saw.clone();
            Box::pin(async move {
                ctx.cancel.cancelled().await;
                saw.store(true, Ordering::SeqCst);
                // Ignores the token's request to stop.
                tokio::time::sleep(Duration::from_secs(3600)).await;
                Ok(json!("never"))
            })
        }),
    );
    let spy = Spy(MockProvider::new().respond_with_blocks(vec![
        tool_use("fast", "echo", json!({"value": "x"})),
        tool_use("slow", "sleepy", json!({})),
    ]));
    let agent = AgentLoop::new(registry)
        .with_model("m")
        .with_cancel_grace(Duration::from_millis(100));
    let cancel = CancellationToken::new();
    let (tx, mut rx) = event_channel(1024);

    let trigger = cancel.clone();
    let watcher = tokio::spawn(async move {
        let mut seen = Vec::new();
        while let Some(e) = rx.recv().await {
            if matches!(&e, AgentEvent::ToolFinished { call_id, .. } if call_id == "fast") {
                trigger.cancel();
            }
            seen.push(e);
        }
        seen
    });
    let mut history = vec![Message::user("go")];
    let outcome = agent
        .run_on(&spy, &mut history, &tx, &cancel)
        .await
        .unwrap();
    drop(tx);
    let events = watcher.await.unwrap();

    assert_eq!(outcome.reason, StopReason::Interrupted);
    assert!(saw_cancel.load(Ordering::SeqCst));
    let results = tool_results(&history);
    assert_eq!(results[0], ("fast".into(), "echoed:x".into(), false));
    assert_eq!(
        results[1],
        ("slow".into(), "Interrupted by user".into(), true)
    );
    assert!(validate_history(&history).is_ok());
    assert!(events.iter().any(|e| matches!(
        e,
        AgentEvent::ToolFinished { call_id, result } if call_id == "slow" && result.is_error
    )));
}

#[tokio::test]
async fn a_handler_that_stops_within_the_grace_period_keeps_its_own_result() {
    let mut registry = Registry::new();
    registry.register(ToolDefinition::new("polite").context_handler(|_, ctx| {
        Box::pin(async move {
            ctx.cancel.cancelled().await;
            Ok(json!("stopped early, 3 lines so far"))
        })
    }));
    let spy =
        Spy(MockProvider::new().respond_with_blocks(vec![tool_use("p", "polite", json!({}))]));
    let agent = AgentLoop::new(registry).with_model("m");
    let cancel = CancellationToken::new();
    tokio::spawn(cancel_after(cancel.clone(), 50));
    let mut history = vec![Message::user("go")];

    let (outcome, _) = run_collect(&agent, &spy, &mut history, &cancel).await;

    assert_eq!(outcome.reason, StopReason::Interrupted);
    assert_eq!(tool_results(&history)[0].1, "stopped early, 3 lines so far");
}

#[tokio::test]
async fn cancel_while_waiting_on_approval_resolves_every_call_as_interrupted() {
    struct Never;
    impl cosmos_llm_tool::Approver for Never {
        fn check<'a>(
            &'a self,
            _req: &'a cosmos_llm_tool::ApprovalRequest,
        ) -> BoxFuture<'a, Decision> {
            Box::pin(std::future::pending())
        }
    }
    let ran = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let spy = Spy(MockProvider::new().respond_with_blocks(vec![
        tool_use("a", "shell", json!({})),
        tool_use("b", "shell", json!({})),
    ]));
    let agent = AgentLoop::new(counting_registry(ran.clone()))
        .with_model("m")
        .with_approver(Arc::new(Never));
    let cancel = CancellationToken::new();
    tokio::spawn(cancel_after(cancel.clone(), 50));
    let mut history = vec![Message::user("go")];

    let (outcome, _) = run_collect(&agent, &spy, &mut history, &cancel).await;

    assert_eq!(outcome.reason, StopReason::Interrupted);
    assert_eq!(ran.load(Ordering::SeqCst), 0);
    assert_eq!(
        tool_results(&history),
        vec![
            ("a".into(), "Interrupted by user".into(), true),
            ("b".into(), "Interrupted by user".into(), true)
        ]
    );
    assert!(validate_history(&history).is_ok());
}

#[tokio::test]
async fn a_thread_keeps_queued_input_after_an_interrupt() {
    let mut thread = Thread::new("t");
    let queue = thread.queue();
    let cancel = CancellationToken::new();
    tokio::spawn(cancel_after(cancel.clone(), 50));
    let hang = Hang(vec![StreamChunk::text("working")]);
    let agent = AgentLoop::new(echo_registry()).with_model("m");
    let handles = RunHandles::new().cancel(cancel);
    let pusher = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        queue.push("wait, one more thing");
    });

    let outcome = thread.send(&agent, &hang, "start", &handles).await.unwrap();
    pusher.await.unwrap();

    assert_eq!(outcome.reason, StopReason::Interrupted);
    assert_eq!(thread.queue().len(), 1);
    assert_eq!(thread.status(), ThreadStatus::Idle);
}
