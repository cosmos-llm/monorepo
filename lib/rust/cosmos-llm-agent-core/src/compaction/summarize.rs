//! The default compactors: summarize with the model, or just drop.

use std::ops::Range;
use std::sync::Arc;

use cosmos_llm::tokens::{estimate_text_tokens, estimate_tokens};
use cosmos_llm::{CompletionRequest, ContentBlock, Message, Role};
use cosmos_llm_registry::{meta, Meta, Plugin};
use cosmos_llm_tool::Completer;
use futures_util::future::BoxFuture;

use super::{CompactionConfig, CompactionOutcome, CompactionPlan, Compactor};
use crate::error::CoreError;

/// Starts every summary message, so a later compaction recognizes it and
/// folds it in instead of summarizing a summary of a summary.
pub const SUMMARY_PREFIX: &str = "[Context summary: earlier messages were compacted]";

const USER_SECTION: &str = "## Recent user messages (verbatim, newest first)";

const SUMMARY_PROMPT: &str = "\
You are compacting the history of a coding-agent session so the work can continue \
in a fresh context. Write a structured summary of the material below. Use these \
headings: Goal; Constraints and preferences from the user; Key decisions; Files \
touched and why; Current state; Next steps; Open problems. Be concrete: keep file \
paths, command names, error messages and identifiers exactly. Do not invent \
anything. If the material begins with an earlier summary, fold it in; the result \
is one summary, not a summary of a summary. Reply with the summary only.";

/// Whether `msg` is a summary written by a compactor here.
pub fn is_summary(msg: &Message) -> bool {
    msg.role == Role::User
        && msg.content.iter().any(|b| match b {
            ContentBlock::Text { text } => text.starts_with(SUMMARY_PREFIX),
            _ => false,
        })
}

fn has_tool_result(msg: &Message) -> bool {
    msg.content
        .iter()
        .any(|b| matches!(b, ContentBlock::ToolResult { .. }))
}

fn has_tool_use(msg: &Message) -> bool {
    msg.content
        .iter()
        .any(|b| matches!(b, ContentBlock::ToolUse { .. }))
}

/// A real user message: not a tool result, not a summary.
fn is_user_turn(msg: &Message) -> bool {
    msg.role == Role::User && !has_tool_result(msg) && !is_summary(msg)
}

fn tokens_of(msg: &Message) -> u32 {
    estimate_tokens(std::slice::from_ref(msg))
}

/// Picks the first index of the verbatim tail.
///
/// A cut at `i` is valid when `history[i]` is not a tool result and
/// `history[i - 1]` requested no tools, so no pair is split. Among valid cuts
/// the earliest one whose tail fits the budget wins, preferring the start of a
/// user turn; the last `keep_turns` user turns are always kept when possible.
fn pick_cut(history: &[Message], plan: &CompactionPlan, cfg: &CompactionConfig) -> Option<usize> {
    let n = history.len();
    let valid =
        |i: usize| i >= 1 && !has_tool_result(&history[i]) && !has_tool_use(&history[i - 1]);
    let cuts: Vec<usize> = (1..n).filter(|&i| valid(i)).collect();
    if cuts.is_empty() {
        return None;
    }
    let mut suffix = vec![0u32; n + 1];
    for i in (0..n).rev() {
        suffix[i] = suffix[i + 1].saturating_add(tokens_of(&history[i]));
    }
    let base = if plan.window > 0 {
        plan.window
    } else {
        plan.used
    };
    let budget = (f64::from(base) * cfg.tail_fraction.clamp(0.0, 1.0)) as u32;
    let fits = |i: &usize| suffix[*i] <= budget;
    let mut cut = cuts
        .iter()
        .copied()
        .filter(fits)
        .find(|&i| is_user_turn(&history[i]))
        .or_else(|| cuts.iter().copied().find(fits))
        .unwrap_or_else(|| *cuts.last().unwrap_or(&1));
    let turn_starts: Vec<usize> = (1..n)
        .filter(|&i| valid(i) && is_user_turn(&history[i]))
        .collect();
    let keep = cfg.keep_turns.max(1) as usize;
    if turn_starts.len() >= keep {
        cut = cut.min(turn_starts[turn_starts.len() - keep]);
    }
    Some(cut)
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let kept: String = s.chars().take(max).collect();
    format!("{kept} [... {} chars cut]", s.chars().count() - max)
}

fn block_text(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .map(|b| match b {
            ContentBlock::Text { text } => text.clone(),
            ContentBlock::Image { .. } => "[image]".to_owned(),
            _ => String::new(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// One message as transcript text, tool results cut hard.
fn render(msg: &Message, cfg: &CompactionConfig) -> String {
    if is_summary(msg) {
        return format!("[earlier summary]\n{}", msg.text());
    }
    let role = match msg.role {
        Role::User => "user",
        Role::Assistant => "assistant",
        _ => "other",
    };
    let mut parts = Vec::new();
    for b in &msg.content {
        match b {
            ContentBlock::Text { text } => parts.push(format!("[{role}] {text}")),
            ContentBlock::Image { .. } => parts.push(format!("[{role}] [image]")),
            ContentBlock::ToolUse { name, input, .. } => parts.push(format!(
                "[tool call {name}] {}",
                truncate_chars(&input.to_string(), 500)
            )),
            ContentBlock::ToolResult {
                content, is_error, ..
            } => {
                let tag = if *is_error {
                    "tool error"
                } else {
                    "tool result"
                };
                parts.push(format!(
                    "[{tag}] {}",
                    truncate_chars(&block_text(content), cfg.tool_result_chars)
                ));
            }
            _ => {}
        }
    }
    parts.join("\n")
}

/// The user's own words in `msgs`, newest first, within `budget` tokens.
fn recent_user_section(msgs: &[Message], budget: u32) -> String {
    let mut used = 0u32;
    let mut items = Vec::new();
    for m in msgs.iter().rev().filter(|m| is_user_turn(m)) {
        let text = m.text();
        if text.trim().is_empty() {
            continue;
        }
        let cost = estimate_text_tokens(&text);
        if used.saturating_add(cost) > budget {
            if items.is_empty() {
                items.push(truncate_chars(&text, (budget as usize).saturating_mul(4)));
            }
            break;
        }
        used += cost;
        items.push(text);
    }
    if items.is_empty() {
        return String::new();
    }
    let body: Vec<String> = items
        .iter()
        .map(|t| format!("- {}", t.replace('\n', "\n  ")))
        .collect();
    format!("\n\n{USER_SECTION}\n\n{}", body.join("\n"))
}

fn summary_message(summary: &str, user_section: &str) -> Message {
    Message::user(format!(
        "{SUMMARY_PREFIX}\n\n{}{user_section}",
        summary.trim()
    ))
}

/// Shared by both compactors: where to cut and what is being replaced.
fn split(
    plan: &CompactionPlan,
    history: &[Message],
    cfg: &CompactionConfig,
) -> Result<usize, CoreError> {
    let cut = pick_cut(history, plan, cfg)
        .ok_or_else(|| CoreError::Setup("nothing to compact: no safe cut point".into()))?;
    if cut == 1 && is_summary(&history[0]) {
        return Err(CoreError::Setup(
            "nothing new to compact since the last summary".into(),
        ));
    }
    Ok(cut)
}

/// Summarizes the dropped range with the model.
pub struct SummarizingCompactor {
    completer: Arc<dyn Completer>,
    model: String,
    config: CompactionConfig,
}

impl SummarizingCompactor {
    /// A compactor that asks `completer` for `model`'s summary.
    pub fn new(
        completer: Arc<dyn Completer>,
        model: impl Into<String>,
        config: CompactionConfig,
    ) -> Self {
        Self {
            completer,
            model: model.into(),
            config,
        }
    }

    /// Splits the transcript into chunks of about `chunk_tokens`.
    fn chunks(&self, dropped: &[Message]) -> Vec<String> {
        let limit = self.config.chunk_tokens.max(256);
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut cur_tokens = 0u32;
        for m in dropped {
            let mut text = render(m, &self.config);
            let t = estimate_text_tokens(&text);
            if t > limit {
                text = truncate_chars(&text, (limit as usize).saturating_mul(4));
            }
            let t = t.min(limit);
            if cur_tokens + t > limit && !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
                cur_tokens = 0;
            }
            cur.push_str(&text);
            cur.push_str("\n\n");
            cur_tokens += t;
        }
        if !cur.is_empty() {
            out.push(cur);
        }
        out
    }

    async fn summarize(
        &self,
        plan: &CompactionPlan,
        dropped: &[Message],
    ) -> Result<String, CoreError> {
        let mut running: Option<String> = None;
        for chunk in self.chunks(dropped) {
            let mut prompt = String::from(SUMMARY_PROMPT);
            if let Some(extra) = plan
                .instructions
                .as_deref()
                .filter(|s| !s.trim().is_empty())
            {
                prompt.push_str("\n\nThe user asked to focus on: ");
                prompt.push_str(extra.trim());
            }
            if let Some(prev) = &running {
                prompt.push_str("\n\nSummary so far (fold it in):\n");
                prompt.push_str(prev);
            }
            prompt.push_str("\n\n--- material ---\n\n");
            prompt.push_str(&chunk);
            let mut req = CompletionRequest::new(self.model.clone(), vec![Message::user(prompt)]);
            req.max_tokens = Some(self.config.summary_max_tokens);
            let resp = self.completer.completion(&req).await?;
            let text = resp
                .choices
                .first()
                .map(|c| c.message.text())
                .unwrap_or_default();
            if text.trim().is_empty() {
                return Err(CoreError::Setup("the summary call returned no text".into()));
            }
            running = Some(text);
        }
        running.ok_or_else(|| CoreError::Setup("nothing to summarize".into()))
    }
}

impl Compactor for SummarizingCompactor {
    fn compact<'a>(
        &'a self,
        plan: &'a CompactionPlan,
        history: &'a [Message],
    ) -> BoxFuture<'a, Result<CompactionOutcome, CoreError>> {
        Box::pin(async move {
            let cut = split(plan, history, &self.config)?;
            let dropped = &history[..cut];
            let summary = self.summarize(plan, dropped).await?;
            let budget = user_budget(plan, &self.config);
            let section = recent_user_section(dropped, budget);
            let range: Range<usize> = 0..cut;
            Ok(CompactionOutcome::new(
                range,
                summary_message(&summary, &section),
            ))
        })
    }
}

fn user_budget(plan: &CompactionPlan, cfg: &CompactionConfig) -> u32 {
    if plan.target > 0 {
        cfg.recent_user_tokens.min(plan.target / 4)
    } else {
        cfg.recent_user_tokens
    }
}

/// Makes no model call: replaces the dropped range with a short note and the
/// user's recent messages.
pub struct DropCompactor {
    config: CompactionConfig,
}

impl DropCompactor {
    /// A drop-only compactor.
    pub fn new(config: CompactionConfig) -> Self {
        Self { config }
    }
}

impl Compactor for DropCompactor {
    fn compact<'a>(
        &'a self,
        plan: &'a CompactionPlan,
        history: &'a [Message],
    ) -> BoxFuture<'a, Result<CompactionOutcome, CoreError>> {
        Box::pin(async move {
            let cut = split(plan, history, &self.config)?;
            let section = recent_user_section(&history[..cut], user_budget(plan, &self.config));
            let note = format!("{} earlier messages were dropped to make room.", cut);
            Ok(CompactionOutcome::new(
                0..cut,
                summary_message(&note, &section),
            ))
        })
    }
}

/// What a strategy gets to build itself from.
#[derive(Clone)]
#[non_exhaustive]
pub struct CompactorContext {
    /// The client for summary calls.
    pub completer: Arc<dyn Completer>,
    /// The model that writes summaries.
    pub model: String,
    /// The `compaction` config section.
    pub config: CompactionConfig,
}

/// A compaction strategy registration, found by `compaction.strategy`.
pub struct CompactorPlugin {
    /// Name and aliases.
    pub meta: Meta,
    /// Builds the compactor.
    pub build: fn(&CompactorContext) -> Arc<dyn Compactor>,
}

impl Plugin for CompactorPlugin {
    fn meta(&self) -> &Meta {
        &self.meta
    }
}

cosmos_llm_registry::inventory::collect!(CompactorPlugin);

fn build_summarize(ctx: &CompactorContext) -> Arc<dyn Compactor> {
    Arc::new(SummarizingCompactor::new(
        Arc::clone(&ctx.completer),
        ctx.model.clone(),
        ctx.config.clone(),
    ))
}

fn build_drop(ctx: &CompactorContext) -> Arc<dyn Compactor> {
    Arc::new(DropCompactor::new(ctx.config.clone()))
}

cosmos_llm_registry::inventory::submit! {
    CompactorPlugin {
        meta: meta!("summarize").builtin().summary("Ask the model for a structured summary; keep the tail and recent user messages"),
        build: build_summarize,
    }
}

cosmos_llm_registry::inventory::submit! {
    CompactorPlugin {
        meta: meta!("drop").builtin().summary("No model call: drop old messages, keep the tail and recent user messages"),
        build: build_drop,
    }
}

/// Builds the compactor registered as `name`.
///
/// # Errors
///
/// Fails when no strategy has that name.
pub fn compactor(name: &str, ctx: &CompactorContext) -> Result<Arc<dyn Compactor>, CoreError> {
    let plugin = cosmos_llm_registry::require::<CompactorPlugin>("compaction strategy", name)
        .map_err(|e| CoreError::Config(e.to_string()))?;
    Ok((plugin.build)(ctx))
}

/// The compactor `CoreBuilder` installs when the host gave none: `None` when
/// compaction is disabled or the strategy is unknown (a warning is pushed).
pub fn default_compactor(
    config: &CompactionConfig,
    completer: &Arc<dyn Completer>,
    session_model: &str,
    warnings: &mut Vec<String>,
) -> Option<Arc<dyn Compactor>> {
    if !config.enabled {
        return None;
    }
    let ctx = CompactorContext {
        completer: Arc::clone(completer),
        model: config
            .model
            .clone()
            .unwrap_or_else(|| session_model.to_owned()),
        config: config.clone(),
    };
    match compactor(&config.strategy, &ctx) {
        Ok(c) => Some(c),
        Err(e) => {
            warnings.push(format!("compaction disabled: {e}"));
            None
        }
    }
}
