//! Compaction (170): cut points, the summary call, folding, and the default
//! wiring in `Core`.

mod common;

use std::sync::Arc;

use common::*;
use cosmos_llm::providers::mock::MockProvider;
use cosmos_llm::{ContentBlock, Message, Role};
use cosmos_llm_agent_core::compaction::is_summary;
use cosmos_llm_agent_core::{
    CompactionConfig, CompactionOutcome, CompactionPlan, CompactionReason, Compactor, Core,
    DropCompactor, Event, Op, SummarizingCompactor,
};
use cosmos_llm_tool::validate_history;
use serde_json::json;

fn plan(used: u32, window: u32, target: u32) -> CompactionPlan {
    serde_json::from_value(json!({
        "reason": "threshold", "used": used, "window": window, "target": target
    }))
    .unwrap()
}

fn tool_turn(i: usize, size: usize) -> Vec<Message> {
    let id = format!("call_{i}");
    vec![
        Message::user(format!("please do step {i}")),
        Message::from_blocks(
            Role::Assistant,
            vec![
                ContentBlock::text(format!("working on {i}")),
                ContentBlock::ToolUse {
                    id: id.clone(),
                    name: "read".into(),
                    input: json!({"path": format!("f{i}.rs")}),
                },
            ],
        ),
        Message::from_blocks(
            Role::User,
            vec![ContentBlock::ToolResult {
                tool_use_id: id,
                content: vec![ContentBlock::text("x".repeat(size))],
                is_error: false,
            }],
        ),
        Message::assistant(format!("done {i}")),
    ]
}

fn history(turns: usize, size: usize) -> Vec<Message> {
    (0..turns).flat_map(|i| tool_turn(i, size)).collect()
}

fn apply(h: &[Message], o: &CompactionOutcome) -> Vec<Message> {
    let mut v = h[..o.replaces.start].to_vec();
    v.push(o.summary.clone());
    v.extend_from_slice(&h[o.replaces.end..]);
    v
}

fn summarizer(mock: MockProvider, cfg: CompactionConfig) -> (Arc<Recording>, SummarizingCompactor) {
    let rec = completer(mock);
    (
        rec.clone(),
        SummarizingCompactor::new(rec, "mock-model", cfg),
    )
}

#[tokio::test]
async fn compacts_under_target_keeps_the_last_user_message_and_stays_valid() {
    let h = history(12, 4_000);
    let before = cosmos_llm::tokens::estimate_tokens(&h);
    let (rec, c) = summarizer(
        MockProvider::new().respond("Goal: finish steps."),
        CompactionConfig::default(),
    );
    let p = plan(before, before * 2, before / 2);
    let out = c.compact(&p, &h).await.unwrap();
    let new = apply(&h, &out);
    validate_history(&new).unwrap();
    assert!(cosmos_llm::tokens::estimate_tokens(&new) < before / 2);
    assert!(is_summary(&new[0]));
    assert!(new[0].text().contains("Goal: finish steps."));
    // The latest turn is verbatim.
    assert_eq!(new.last().unwrap().text(), "done 11");
    assert!(new.iter().any(|m| m.text() == "please do step 11"));
    // Older user messages ride along in the summary, newest first.
    let t = new[0].text();
    assert!(t.find("please do step 5").unwrap() < t.find("please do step 4").unwrap());
    // Tool output was cut hard before the summarizer saw it.
    let req = rec.last_request().unwrap();
    assert!(req.messages[0].text().len() < 20_000);
    assert!(req.messages[0].text().contains("chars cut"));
}

#[tokio::test]
async fn the_cut_never_separates_a_tool_pair() {
    let h = history(8, 300);
    let total = cosmos_llm::tokens::estimate_tokens(&h);
    for fraction in [0.0, 0.05, 0.1, 0.2, 0.33, 0.5, 0.8] {
        for keep in [1, 2, 3] {
            let mut cfg = CompactionConfig::default();
            cfg.tail_fraction = fraction;
            cfg.keep_turns = keep;
            let (_r, c) = summarizer(MockProvider::new().respond("S"), cfg);
            let out = c
                .compact(&plan(total, total * 2, total / 2), &h)
                .await
                .unwrap();
            validate_history(&apply(&h, &out))
                .unwrap_or_else(|e| panic!("fraction {fraction} keep {keep}: {e}"));
        }
    }
}

#[tokio::test]
async fn one_long_turn_is_cut_between_steps() {
    // A single user turn with many tool rounds: the cut falls inside it.
    let mut h = vec![Message::user("refactor everything")];
    for i in 0..10 {
        h.extend(tool_turn(i, 500).into_iter().skip(1));
    }
    let total = cosmos_llm::tokens::estimate_tokens(&h);
    let (_r, c) = summarizer(
        MockProvider::new().respond("S"),
        CompactionConfig::default(),
    );
    let out = c
        .compact(&plan(total, total * 2, total / 2), &h)
        .await
        .unwrap();
    assert!(out.replaces.end > 1 && out.replaces.end < h.len());
    validate_history(&apply(&h, &out)).unwrap();
}

#[tokio::test]
async fn a_second_compaction_folds_the_earlier_summary() {
    let h = history(10, 1_000);
    let total = cosmos_llm::tokens::estimate_tokens(&h);
    let (_r, c) = summarizer(
        MockProvider::new().respond("FIRST SUMMARY"),
        CompactionConfig::default(),
    );
    let first = apply(
        &h,
        &c.compact(&plan(total, total * 2, total / 2), &h)
            .await
            .unwrap(),
    );
    let mut again = first.clone();
    again.extend(history(6, 1_000));
    let total = cosmos_llm::tokens::estimate_tokens(&again);
    let (rec, c) = summarizer(
        MockProvider::new().respond("SECOND SUMMARY"),
        CompactionConfig::default(),
    );
    let out = c
        .compact(&plan(total, total * 2, total / 2), &again)
        .await
        .unwrap();
    let new = apply(&again, &out);
    validate_history(&new).unwrap();
    assert_eq!(new.iter().filter(|m| is_summary(m)).count(), 1);
    assert!(new[0].text().contains("SECOND SUMMARY"));
    assert!(!new[0].text().contains("FIRST SUMMARY"));
    // The model was shown the earlier summary to fold in.
    assert!(rec.last_request().unwrap().messages[0]
        .text()
        .contains("FIRST SUMMARY"));
}

#[tokio::test]
async fn nothing_new_since_the_last_summary_is_an_error() {
    let h = vec![
        Message::user(format!(
            "{}\n\nS",
            cosmos_llm_agent_core::compaction::SUMMARY_PREFIX
        )),
        Message::assistant("ok"),
    ];
    let (rec, c) = summarizer(
        MockProvider::new().respond("S"),
        CompactionConfig::default(),
    );
    assert!(c.compact(&plan(100, 1000, 500), &h).await.is_err());
    assert!(rec.requests().is_empty());
}

#[tokio::test]
async fn an_oversized_history_is_summarized_in_chunks() {
    let h = history(10, 1_500);
    let total = cosmos_llm::tokens::estimate_tokens(&h);
    let mut cfg = CompactionConfig::default();
    cfg.chunk_tokens = 1_000;
    let (rec, c) = summarizer(
        MockProvider::new()
            .respond("part one")
            .respond("part two")
            .respond("part three")
            .respond("part four")
            .respond("part five")
            .respond("part six")
            .respond("part seven")
            .respond("part eight")
            .respond("part nine"),
        cfg,
    );
    let out = c
        .compact(&plan(total, total * 2, total / 2), &h)
        .await
        .unwrap();
    let calls = rec.requests();
    assert!(calls.len() > 1);
    // Each later call folds in the summary so far.
    assert!(calls[1].messages[0].text().contains("part one"));
    assert!(out.summary.text().contains(&format!(
        "part {}",
        ["one", "two", "three", "four", "five", "six", "seven", "eight", "nine"][calls.len() - 1]
    )));
}

#[tokio::test]
async fn the_drop_strategy_makes_no_model_call() {
    let h = history(10, 1_000);
    let total = cosmos_llm::tokens::estimate_tokens(&h);
    let c = DropCompactor::new(CompactionConfig::default());
    let out = c
        .compact(&plan(total, total * 2, total / 2), &h)
        .await
        .unwrap();
    let new = apply(&h, &out);
    validate_history(&new).unwrap();
    assert!(is_summary(&new[0]));
}

#[tokio::test]
async fn instructions_reach_the_summary_prompt() {
    let h = history(6, 500);
    let total = cosmos_llm::tokens::estimate_tokens(&h);
    let (rec, c) = summarizer(
        MockProvider::new().respond("S"),
        CompactionConfig::default(),
    );
    let mut p = plan(total, total * 2, total / 2);
    p.instructions = Some("focus on the auth bug".into());
    p.reason = CompactionReason::Manual;
    c.compact(&p, &h).await.unwrap();
    assert!(rec.last_request().unwrap().messages[0]
        .text()
        .contains("focus on the auth bug"));
}

#[tokio::test]
async fn core_uses_the_summarizer_by_default() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let rec = completer(
        MockProvider::new()
            .respond("one")
            .respond("Goal: say hi.")
            .respond("two"),
    );
    let (h, mut rx) = Core::builder(config(cwd.path(), sess.path()))
        .completer(rec.clone())
        .tools(registry())
        .start()
        .unwrap();
    h.user_text("hi").await.unwrap();
    until(&mut rx, run_finished).await;
    h.send(Op::Compact {
        instructions: Some("be brief".into()),
    })
    .await
    .unwrap();
    until(&mut rx, |e| matches!(e, Event::CompactionFinished { .. })).await;
    h.user_text("more").await.unwrap();
    until(&mut rx, run_finished).await;
    let last = rec.last_request().unwrap();
    assert!(last
        .messages
        .iter()
        .any(|m| m.text().contains("Goal: say hi.")));
    assert!(rec.requests()[1].messages[0].text().contains("be brief"));
}

#[tokio::test]
async fn compaction_can_be_turned_off_or_swapped() {
    let (cwd, sess) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut cfg = config(cwd.path(), sess.path());
    let mut cc = CompactionConfig::default();
    cc.enabled = false;
    cfg.compaction = cc;
    let (h, mut rx) = Core::builder(cfg)
        .completer(completer(MockProvider::new().respond("one")))
        .tools(registry())
        .start()
        .unwrap();
    h.user_text("hi").await.unwrap();
    until(&mut rx, run_finished).await;
    h.send(Op::Compact { instructions: None }).await.unwrap();
    let ev = until(&mut rx, |e| matches!(e, Event::Error { .. })).await;
    assert!(matches!(ev.last(), Some(Event::Error { .. })));

    // `drop` is a registered strategy; an unknown one warns and disables.
    let mut cfg = config(cwd.path(), sess.path());
    let mut cc = CompactionConfig::default();
    cc.strategy = "drop".into();
    cfg.compaction = cc;
    let rec = completer(MockProvider::new().respond("one"));
    let (h, mut rx) = Core::builder(cfg)
        .completer(rec.clone())
        .tools(registry())
        .start()
        .unwrap();
    h.user_text("hi").await.unwrap();
    until(&mut rx, run_finished).await;
    h.send(Op::Compact { instructions: None }).await.unwrap();
    until(&mut rx, |e| matches!(e, Event::CompactionFinished { .. })).await;
    assert_eq!(rec.requests().len(), 1);

    let mut cfg = config(cwd.path(), sess.path());
    let mut cc = CompactionConfig::default();
    cc.strategy = "nope".into();
    cfg.compaction = cc;
    let (_h, mut rx) = Core::builder(cfg)
        .completer(completer(MockProvider::new()))
        .tools(registry())
        .start()
        .unwrap();
    let ev = until(&mut rx, |e| matches!(e, Event::Warning { .. })).await;
    assert!(matches!(ev.last(), Some(Event::Warning { message }) if message.contains("nope")));
}
