//! Golden files for the IRs of commands, hooks and subagents (230, 240).
//! Same contract as `golden.rs`: JSON and TrongoScript fixtures that both
//! deserialize to the samples built here and round-trip.
//!
//! Regenerate with `UPDATE_GOLDEN=1 cargo test --test golden_ext`, then read
//! the diff.

use std::path::PathBuf;

use cosmos_llm_agent_core::trongo::{from_trongo, to_trongo};
use cosmos_llm_agent_core::{
    AgentSpec, ApprovalMode, CommandSpec, Event, HookDecision, HookEvent, HookPayload,
    HookResponse, HookSpec, Layer, PayloadFormat,
};
use cosmos_llm_tool::AgentEvent;
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
    assert_eq!(
        serde_json::to_value(&from_json).unwrap(),
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
fn command_spec() {
    let mut full = CommandSpec::new("db:migrate", "Run the migration for $ARGUMENTS");
    full.description = "Review the staged diff".into();
    full.args = Some("[focus]".into());
    full.model = Some("claude-sonnet-5-5".into());
    full.allowed_tools = vec!["read".into(), "grep".into(), "bash".into()];
    full.layer = Some(Layer::Project);
    check(
        "command_spec",
        vec![CommandSpec::new("review", "Review the diff."), full],
    );
}

#[test]
fn hook_spec() {
    let mut trongo_hook = HookSpec::new(HookEvent::Stop, "scripts/check-tests.sh");
    trongo_hook.payload_format = PayloadFormat::Trongo;
    check(
        "hook_spec",
        vec![
            HookSpec::new(HookEvent::PostToolUse, "cargo fmt -- $COSMOS_FILE_PATHS")
                .matcher("edit|multi_edit|write")
                .timeout_ms(10_000),
            HookSpec::new(HookEvent::PreToolUse, "scripts/guard.sh").matcher("bash"),
            HookSpec::new(HookEvent::UserPromptSubmit, "date"),
            trongo_hook,
            HookSpec::new(HookEvent::SessionStart, "git status --short"),
        ],
    );
}

#[test]
fn hook_payload() {
    let mut pre = HookPayload::new(HookEvent::PreToolUse);
    pre.session_id = "s-1".into();
    pre.cwd = "/work".into();
    pre.tool = Some("write".into());
    pre.input = Some(json!({"file_path": "a.rs", "content": "fn main() {}"}));
    let mut post = pre.clone();
    post.event = HookEvent::PostToolUse;
    post.result = Some(json!("wrote 1 line"));
    let mut stop = HookPayload::new(HookEvent::Stop);
    stop.session_id = "s-1".into();
    stop.cwd = "/work".into();
    stop.assistant_message = Some("All done.".into());
    stop.stop_hook_active = true;
    let mut prompt = HookPayload::new(HookEvent::UserPromptSubmit);
    prompt.prompt = Some("fix the bug".into());
    check("hook_payload", vec![pre, post, stop, prompt]);
}

#[test]
fn hook_response() {
    let mut block = HookResponse::default();
    block.decision = Some(HookDecision::Block);
    block.reason = Some("migrations/ is read-only".into());
    let mut ctx = HookResponse::default();
    ctx.additional_context = Some("formatted 2 files".into());
    let mut rewrite = HookResponse::default();
    rewrite.decision = Some(HookDecision::Allow);
    rewrite.updated_input = Some(json!({"command": "ls"}));
    check(
        "hook_response",
        vec![HookResponse::default(), block, ctx, rewrite],
    );
}

#[test]
fn agent_spec() {
    let mut full = AgentSpec::new("review-rust", "You review rust code.");
    full.description = "Reviews rust changes".into();
    full.tools = Some(vec!["read".into(), "glob".into(), "grep".into()]);
    full.model = Some("claude-haiku-5-5".into());
    full.max_steps = Some(30);
    full.mode = Some(ApprovalMode::ReadOnly);
    full.layer = Some(Layer::User);
    check(
        "agent_spec",
        vec![AgentSpec::general(), AgentSpec::explore(), full],
    );
}

#[test]
fn subagent_event() {
    check(
        "subagent_events",
        vec![
            Event::Subagent {
                call_id: "toolu_1".into(),
                agent: "explore".into(),
                inner: Box::new(AgentEvent::TextDelta {
                    text: "looking".into(),
                }),
            },
            Event::Subagent {
                call_id: "toolu_1/toolu_2".into(),
                agent: "general".into(),
                inner: Box::new(AgentEvent::ToolStarted {
                    call_id: "toolu_9".into(),
                }),
            },
        ],
    );
}
