# 020 — Reasoning and thinking

**Status:** implemented — `Reasoning`/`ReasoningEffort`/`ThinkingDelta` in `reasoning.rs`, `with_reasoning`, Anthropic/OpenAI/OpenRouter request mapping, thinking streamed and kept as signed blocks, `Usage::reasoning_tokens`, local rejection for models the table knows lack reasoning. Deferred: OpenRouter streaming carries one thinking fragment per chunk, so a chunk holding both text and a signature surfaces only the text (non-streaming replay is complete); OpenAI reasoning models still get `max_tokens`, not `max_completion_tokens`.

**Crate:** `cosmos-llm` · **Depends on:** 010 · **Blocks:** nothing (nice to have for 060)

## Problem

There is no way to ask for extended thinking or set reasoning effort, and the
output is thrown away when it arrives. `parse_anthropic_event` ignores
`thinking_delta` and `signature_delta` (`providers/anthropic.rs:466`). On
tool-using turns Anthropic then rejects the replayed history once thinking is
enabled, because the assistant turn is missing its thinking block.

A TUI also wants to show thinking as it streams (collapsed by default), and
to show that the model is thinking rather than stalled.

## Design

### Request

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum Reasoning {
    Off,
    /// Provider-neutral effort. Mapped per provider below.
    Effort(ReasoningEffort),
    /// Explicit token budget; Anthropic only, others map to the nearest effort.
    Budget(u32),
}

pub enum ReasoningEffort { Minimal, Low, Medium, High }

impl CompletionRequest {
    pub fn with_reasoning(self, r: Reasoning) -> Self;
}
```

| Provider | Mapping |
|---|---|
| Anthropic | `thinking: {type: "enabled", budget_tokens}`. Effort → budget table (e.g. low 2k, medium 8k, high 24k). `max_tokens` must exceed the budget; raise it automatically and say so in the docs. Temperature must be unset. |
| OpenAI | `reasoning_effort: "low" \| "medium" \| "high"`. Chat completions returns no reasoning text, only `completion_tokens_details.reasoning_tokens`. |
| OpenRouter | `reasoning: {effort}` or `reasoning: {max_tokens}`. Returns `reasoning` text and `reasoning_details` on the message and in deltas. |

Model support differs. Whether a model accepts reasoning at all comes from
model metadata (050). Sending it to a model that does not support it should
fail locally with `CosmosError::Configuration`, not return a 400.

### Response and stream

- `StreamChunk` gains `thinking: Option<ThinkingDelta>`:

  ```rust
  pub enum ThinkingDelta {
      Text { index: u32, text: String },
      Signature { index: u32, signature: String },
      Redacted { index: u32, data: String },
  }
  ```

- Anthropic: `content_block_start` with `type: thinking` / `redacted_thinking`
  opens a block; `thinking_delta` and `signature_delta` fill it.
- OpenRouter: `delta.reasoning` is text; `reasoning_details` carries the
  signature or encrypted payload. Store it so it can be replayed.
- `StreamAccumulator` produces `ContentBlock::Thinking { text, signature }`
  in the right position (010).
- `Usage` gains `reasoning_tokens: Option<u32>`, filled from OpenAI's
  `completion_tokens_details` and OpenRouter's equivalent. It is a subset of
  `completion_tokens`, not an addition to it. Document that.

### Replay rules

- Anthropic: send thinking blocks back unmodified on the assistant turn that
  holds the `tool_use`. Earlier turns' thinking may be dropped, and the API
  strips it anyway, so there is no need to make compaction (170) preserve it.
- OpenAI chat completions: nothing to replay.
- OpenRouter: replay `reasoning_details` on the assistant message when present.

## Tests

- Anthropic SSE fixture with thinking → signature → text → tool_use. The
  accumulator yields four blocks in that order, with the signature attached.
- `with_reasoning(Effort(High))` on Anthropic sets `budget_tokens` and raises
  `max_tokens` above it.
- OpenAI usage with `reasoning_tokens` lands in `Usage::reasoning_tokens`.
- Mock provider (`providers/mock.rs`) can script thinking chunks, so loop
  tests in 060 can cover them.

## References

- `pi-mono/packages/ai/src/providers/anthropic.ts`: thinking + signature handling.
- `codex/codex-rs/protocol/src/config_types.rs`: `ReasoningEffort`.
- `crush/internal/agent`: think/reasoning plumbing per provider.
