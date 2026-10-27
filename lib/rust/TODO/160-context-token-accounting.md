# 160 — Token accounting

**Status:** implemented — the `cosmos-llm` piece only, `cosmos_llm::tokens` (`estimate_tokens`, `TokenEstimator`, `CharsEstimator`, `estimate_text_tokens`). Not done: `Client::count_tokens`, the `tiktoken-rs` estimator, and everything in `cosmos-llm-agent-core`. Also in `cosmos-llm-agent-core`: `ContextBudget`/`Pressure`/`ContextTracker` (provider-anchored estimate), `Event::ContextUsage` after every response, an overflow check before the call, and one compact-and-retry on `ContextLength`; the `CompactionPlan` IR and `Compactor` seam are stubbed for 170. Deferred: `Client::count_tokens` and the pressure check between steps of one run (the loop has no pre-request hook; mid-run only reports).

**Crate:** `cosmos-llm-agent-core` (with a small piece in `cosmos-llm`) · **Depends on:** 030, 050 · **Blocks:** 170

## Problem

To compact before the provider rejects a request, and to show "context 72%",
we need the size of the next request before we send it. Today the only number
is `Usage::prompt_tokens` from the previous response, after the fact.

## Design

### Use the provider's count as the anchor

Exact local tokenization is not available for Anthropic models, and keeping
tokenizers for every provider is a maintenance burden. Do what codex and
pi-mono do:

```
estimate(next request) =
    last_response.usage.prompt_tokens         // exact, includes cached (030)
  + last_response.usage.completion_tokens     // the assistant message now in history
  + estimate(messages appended since)         // tool results, new user input
```

Only the tail is estimated, and the tail is small relative to the total, so
the error stays small.

### Estimator

```rust
pub trait TokenEstimator: Send + Sync {
    fn estimate(&self, msgs: &[Message]) -> u32;
}
```

Default: `ceil(chars / 4)` for text, a fixed cost per image (Anthropic:
roughly `w*h/750`, capped around 1600), plus a small per-message overhead.
Optional `tiktoken-rs` implementation for OpenAI models behind a feature. It
is exact there and costs a dependency.

Anthropic has a free `count_tokens` endpoint. Expose it as
`Client::count_tokens(&CompletionRequest)` for callers who want exactness
before an expensive step such as deciding to compact. Do not call it every
turn.

### Budget

```rust
pub struct ContextBudget {
    pub window: u32,          // ModelInfo::context_window (050)
    pub reserve_output: u32,  // max_tokens for the reply
    pub compact_at: f32,      // e.g. 0.85 of (window - reserve_output)
}

pub enum Pressure { Ok, Compact, Overflow }
```

Checked before each model call in the thread driver (070). `Compact` triggers
170. `Overflow` (even compaction cannot fit, e.g. a giant pasted file) fails
the turn with a clear message rather than a provider 400.

Also handle the provider saying "prompt too long" anyway (estimate was off):
classify that error in `CosmosError` (`ContextLengthExceeded`), compact, and
retry once.

### Reporting

Emit `AgentEvent::ContextUsage { used, window }` after each turn for the
status line.

## Tests

- Anchor + tail estimate is within 5% of a recorded real `prompt_tokens` on a
  fixture session.
- `Pressure::Compact` crosses at the configured threshold.
- Anthropic and OpenAI "too long" error bodies map to
  `ContextLengthExceeded`.

## References

- `codex/codex-rs/core/src/compact_token_budget.rs`, `context_manager/`.
- `pi-mono/packages/coding-agent/src/core/compaction/`.
- `goose/crates/goose-context-management/src/model.rs`.
