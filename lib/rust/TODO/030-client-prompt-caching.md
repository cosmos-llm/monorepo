# 030 — Prompt caching

**Status:** implemented — `CacheHint`, `Message::with_cache`, `CompletionRequest::with_cache_tools`, `apply_default_cache_breakpoints`, Anthropic `cache_control` (max four, oldest trimmed) and OpenRouter content-part forwarding, `Usage::cache_read_tokens`/`cache_write_tokens` with `prompt_tokens` normalized to include cached input. Deferred: `TokenUsage` cache fields in `cosmos-llm-tool` (loop work, 060+), and OpenRouter tool-list breakpoints.

**Crate:** `cosmos-llm` · **Depends on:** 010 · **Blocks:** nothing, but cost scales badly without it

## Problem

A coding session resends the system prompt, the tool schemas, and the whole
history every turn. Fifty turns into a session that can be 100k+ input tokens
per request. With caching most of that is billed at a fraction of the price,
and time to first token drops too.

- **Anthropic** caches only up to blocks marked `cache_control: {type:
  "ephemeral"}` (at most four breakpoints per request).
- **OpenAI** caches prefixes automatically. Nothing to send, but
  `prompt_tokens_details.cached_tokens` should be read.
- **OpenRouter** passes `cache_control` through to Anthropic models and
  reports cached tokens.

Today we send no markers and read no cache counters. `Usage` has
`prompt_tokens`, `completion_tokens`, `total_tokens`, `cost` and nothing else.

## Design

### Markers

```rust
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum CacheHint {
    Ephemeral,
    /// Anthropic's 1h TTL option.
    EphemeralLong,
}
```

Attach it to the last block of a message (`Message::cache`, see 010), to the
system prompt, and to the tool list (`CompletionRequest::cache_tools`).
Providers that cannot use the hint ignore it.

### Placement policy

Leave placement to the caller, but ship a helper because nearly every agent
wants the same thing:

```rust
pub fn apply_default_cache_breakpoints(req: &mut CompletionRequest);
```

It marks:

1. the tool definitions,
2. the system prompt,
3. the last message of the previous turn,
4. the newest message.

Breakpoints 3 and 4 move each turn. That way the history prefix written last
turn is read this turn. This is the pattern Claude Code, pi-mono and goose
use.

It stays a helper, not automatic behavior, because a caller doing one-shot
calls (`cosmos-llm-predict`) pays a 25% write premium for a cache it will
never read.

### Usage

```rust
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub cache_read_tokens: Option<u32>,
    pub cache_write_tokens: Option<u32>,
    pub reasoning_tokens: Option<u32>, // 020
    pub cost: Option<f64>,
}
```

Anthropic reports `input_tokens` **excluding** cache reads and writes, while
OpenAI's `prompt_tokens` **includes** cached tokens. Pick one meaning and
normalize in the providers. Recommendation: `prompt_tokens` = total input
tokens the model saw, including cached. That is what context accounting (160)
needs. Document it on the field.

The Anthropic streaming path reads input usage from `message_start`
(`state.prompt_tokens`). The cache fields are in the same object.

`TokenUsage` in `cosmos-llm-tool/src/agent.rs:57` gets the same two fields.

## Tests

- Anthropic body: helper puts `cache_control` on the last tool, the system
  block, and the two expected messages, and never more than four.
- Anthropic `message_start` with `cache_read_input_tokens` and
  `cache_creation_input_tokens` → `Usage` with normalized `prompt_tokens`.
- OpenAI `prompt_tokens_details.cached_tokens` → `cache_read_tokens`.
- OpenAI request body never contains `cache_control`.

## References

- `pi-mono/packages/ai/src/providers/anthropic.ts`: breakpoint placement.
- `goose/crates/goose-providers`: Anthropic cache control.
- `codex/codex-rs/protocol/src/response_usage.rs`: usage fields.
