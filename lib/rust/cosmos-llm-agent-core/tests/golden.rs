//! Golden files: every IR has a JSON and a TrongoScript fixture holding one of
//! each variant. Both must deserialize to the samples built here, and both
//! must round-trip.
//!
//! Regenerate the fixtures with `UPDATE_GOLDEN=1 cargo test --test golden`,
//! then read the diff: the files are the wire contract.

use std::path::PathBuf;

use cosmos_llm::{ContentBlock, Message, Reasoning, ReasoningEffort};
use cosmos_llm_agent_core::trongo::{from_trongo, to_trongo};
use cosmos_llm_agent_core::{
    ApprovalMode, BlobRef, CompactionPlan, CoreConfig, DisplayItem, Envelope, Event, FileChange,
    Fork, Op, PolicySpec, Pressure, Record, Rule, PROTOCOL_VERSION, RECORD_VERSION,
};
use cosmos_llm_coding_tools::diff::ChangeKind;
use cosmos_llm_coding_tools::FileDiff;
use cosmos_llm_tool::{
    AgentEvent, ApprovalScope, Decision, StopReason, TokenUsage, ToolHints, ToolOutcome, TurnStop,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::json;

fn golden(name: &str, ext: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.{ext}"))
}

fn check<T>(name: &str, samples: Vec<T>)
where
    T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        let j = serde_json::to_string_pretty(&samples).unwrap();
        std::fs::write(golden(name, "json"), format!("{j}\n")).unwrap();
        std::fs::write(golden(name, "trongo"), to_trongo(&samples).unwrap()).unwrap();
    }
    let json_src = std::fs::read_to_string(golden(name, "json")).unwrap();
    let from_json: Vec<T> = serde_json::from_str(&json_src).unwrap();
    assert_eq!(
        from_json, samples,
        "{name}: JSON fixture differs from the samples"
    );
    let again = serde_json::to_value(&from_json).unwrap();
    assert_eq!(
        again,
        serde_json::from_str::<serde_json::Value>(&json_src).unwrap()
    );

    let trongo_src = std::fs::read_to_string(golden(name, "trongo")).unwrap();
    let from_t: Vec<T> = from_trongo(&trongo_src).unwrap();
    assert_eq!(
        from_t, samples,
        "{name}: trongo fixture differs from the samples"
    );
    let dumped = to_trongo(&samples).unwrap();
    assert_eq!(from_trongo::<Vec<T>>(&dumped).unwrap(), samples);
    assert_eq!(
        dumped.trim(),
        trongo_src.trim(),
        "{name}: trongo golden is stale"
    );
}

#[test]
fn ops() {
    check(
        "ops",
        vec![
            Op::UserInput {
                blocks: vec![ContentBlock::text("fix the bug")],
            },
            Op::Interrupt,
            Op::Approve {
                call_id: "call_1".into(),
                decision: Decision::Allow,
                remember: false,
            },
            Op::Approve {
                call_id: "call_2".into(),
                decision: Decision::AllowAlways {
                    scope: ApprovalScope::Tool {
                        name: "bash".into(),
                    },
                },
                remember: true,
            },
            Op::Approve {
                call_id: "call_3".into(),
                decision: Decision::deny("not that file"),
                remember: false,
            },
            Op::SetModel {
                model: "claude-sonnet-4".into(),
            },
            Op::SetMode {
                mode: ApprovalMode::AutoEdit,
            },
            Op::SetReasoning {
                reasoning: Reasoning::Effort {
                    effort: ReasoningEffort::High,
                },
            },
            Op::Compact {
                instructions: Some("keep the parser notes".into()),
            },
            Op::Undo { turn: Some(2) },
            Op::TruncateTo { message: 5 },
            Op::RunCommand {
                name: "review".into(),
                args: "src/lib.rs".into(),
            },
            Op::Shutdown,
        ],
    );
}

#[test]
fn events() {
    let usage = TokenUsage {
        prompt_tokens: 1200,
        completion_tokens: 80,
        total_tokens: 1280,
        steps: 1,
        ..TokenUsage::default()
    };
    check(
        "events",
        vec![
            Event::SessionConfigured {
                protocol: PROTOCOL_VERSION,
                session_id: "67e55044-10b1-426f-9247-bb680e5fe0c8".into(),
                model: "claude-sonnet-4".into(),
                cwd: "/work/repo".into(),
                mode: ApprovalMode::Ask,
                context_window: Some(200_000),
                log_path: Some("/data/sessions/abc/s.jsonl".into()),
            },
            Event::HistoryLoaded {
                items: vec![DisplayItem::Message {
                    id: 1,
                    message: Message::user("hi"),
                    usage: None,
                    in_context: true,
                }],
            },
            Event::Agent(AgentEvent::TextDelta {
                text: "Hello".into(),
            }),
            Event::Agent(AgentEvent::ToolFinished {
                call_id: "call_1".into(),
                result: ToolOutcome::text("ok").with_display(json!({"lines": 3})),
            }),
            Event::Agent(AgentEvent::TurnFinished {
                turn: 1,
                stop: TurnStop::EndTurn,
            }),
            Event::ApprovalRequested {
                call_id: "call_2".into(),
                tool: "edit".into(),
                input: json!({"file_path": "src/lib.rs"}),
                preview: Some(json!({"diff": "@@ -1 +1 @@"})),
                hints: ToolHints::new().destructive(true).paths(["src/lib.rs"]),
            },
            Event::ContextUsage {
                used: 1280,
                window: 200_000,
                pressure: Pressure::Ok,
            },
            Event::CompactionStarted {
                reason: cosmos_llm_agent_core::CompactionReason::Threshold,
            },
            Event::CompactionFinished {
                before: 160_000,
                after: 12_000,
            },
            Event::TurnDiff {
                files: vec![FileDiff::binary("logo.png", ChangeKind::Created)],
            },
            Event::Undone {
                turn: 2,
                reverted: vec!["a.rs".into()],
                skipped: vec!["b.rs".into()],
            },
            Event::ModelChanged {
                model: "gpt-5".into(),
                context_window: None,
            },
            Event::ModeChanged {
                mode: ApprovalMode::FullAuto,
            },
            Event::RunFinished {
                reason: Some(StopReason::Finished),
                steps: 3,
                usage,
            },
            Event::Warning {
                message: "careful".into(),
            },
            Event::Error {
                message: "boom".into(),
                fatal: false,
            },
            Event::ShutdownComplete,
        ],
    );
}

#[test]
fn records() {
    let ts = "2026-10-27T14:03:11Z".parse().unwrap();
    let id = "67e55044-10b1-426f-9247-bb680e5fe0c8".parse().unwrap();
    let parent = "11111111-2222-4333-8444-555555555555".parse().unwrap();
    check(
        "records",
        vec![
            Record::SessionStart {
                v: RECORD_VERSION,
                id,
                cwd: "/work/repo".into(),
                model: "claude-sonnet-4".into(),
                started_at: ts,
                parent: Some(Fork::new(parent, Some(3))),
            },
            Record::Message {
                id: 1,
                message: Message::user("hello"),
                usage: None,
            },
            Record::Message {
                id: 2,
                message: Message::assistant("hi there"),
                usage: Some(TokenUsage {
                    prompt_tokens: 10,
                    completion_tokens: 3,
                    total_tokens: 13,
                    steps: 1,
                    ..TokenUsage::default()
                }),
            },
            Record::ToolOutcome {
                call_id: "call_1".into(),
                display: Some(json!({"exit_code": 0})),
            },
            Record::Compaction {
                replaces: 1..5,
                summary: 6,
                except: vec![3],
            },
            Record::ModelChanged {
                model: "gpt-5".into(),
            },
            Record::Truncated { after: 4 },
            Record::FileChanges {
                turn: 1,
                files: vec![
                    FileChange::new(
                        "src/lib.rs",
                        Some(BlobRef("ab12".into())),
                        Some("cd34".into()),
                    ),
                    FileChange::new("new.rs", None, Some("ef56".into())),
                ],
            },
        ],
    );
}

#[test]
fn display_items() {
    check(
        "display_items",
        vec![
            DisplayItem::Message {
                id: 1,
                message: Message::user("a"),
                usage: None,
                in_context: false,
            },
            DisplayItem::ToolDisplay {
                call_id: "c".into(),
                display: json!({"x": 1}),
            },
            DisplayItem::Compaction {
                replaces: 1..3,
                summary: 4,
            },
            DisplayItem::Truncated { after: 2 },
            DisplayItem::ModelChanged { model: "m".into() },
        ],
    );
}

#[test]
fn policy_and_config() {
    check(
        "policy_spec",
        vec![PolicySpec::new(ApprovalMode::ReadOnly)
            .allow(Rule::tool("bash").command("cargo test*"))
            .allow(Rule::tool("edit").path("docs/**"))
            .deny(Rule::tool("bash").command("git push*"))],
    );
    let mut cfg = CoreConfig::default();
    cfg.model = Some("claude-sonnet-4".into());
    cfg.permissions = PolicySpec::new(ApprovalMode::AutoEdit);
    cfg.context.window = Some(100_000);
    check("core_config", vec![cfg]);
    let plan: CompactionPlan = serde_json::from_value(json!({
        "reason": "threshold", "used": 150000, "window": 200000, "target": 70000
    }))
    .unwrap();
    check("compaction_plan", vec![plan]);
    let mut cc = cosmos_llm_agent_core::CompactionConfig::default();
    cc.model = Some("claude-haiku-4".into());
    cc.keep_turns = 3;
    check(
        "compaction_config",
        vec![cosmos_llm_agent_core::CompactionConfig::default(), cc],
    );
}

#[test]
fn exec_results() {
    use cosmos_llm_agent_core::{ExecResult, ExecStop};
    let mut done = ExecResult::new(ExecStop::Finished);
    done.result = "all tests pass".into();
    done.session_id = "0b9a3c9e-0000-4000-8000-000000000001".into();
    done.turns = 4;
    done.usage.prompt_tokens = 1200;
    done.usage.completion_tokens = 300;
    done.usage.total_tokens = 1500;
    done.usage.steps = 4;
    done.cost = Some(0.0125);
    let mut failed = ExecResult::new(ExecStop::Error);
    failed.session_id = "0b9a3c9e-0000-4000-8000-000000000002".into();
    failed.error = Some("provider unavailable".into());
    failed.denied = vec!["write {\"file_path\":\"a.txt\"}".into()];
    let limited = ExecResult::new(ExecStop::MaxTurns);
    check("exec_result", vec![done, failed, limited]);
}

#[test]
fn envelope_carries_the_version() {
    let line = serde_json::to_string(&Envelope::new(Event::ShutdownComplete)).unwrap();
    assert_eq!(line, r#"{"v":1,"event":"shutdown_complete"}"#);
    let back: Envelope<Event> = serde_json::from_str(&line).unwrap();
    assert_eq!(back.v, 1);
    let op: Envelope<Op> =
        serde_json::from_str(r#"{"v":1,"op":"user_input","blocks":[{"type":"text","text":"x"}]}"#)
            .unwrap();
    assert!(matches!(op.body, Op::UserInput { .. }));
}
