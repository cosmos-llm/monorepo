# 010 — Content blocks in `Message`

**Status:** implemented — `ContentBlock`/`ImageSource`/`Role` in `content.rs`, `Message` rebuilt on blocks with the old serde shape still accepted, all three providers and `StreamAccumulator` lowered/rebuilt block by block. Deferred: `CompletionRequest::system` as its own field (a cached system `Message` is hoisted by the Anthropic provider instead), `cosmos-llm-adapter` reusing `Message` (it has no `cosmos-llm` dependency), and the 0.2 version bump.

**Crate:** `cosmos-llm` · **Depends on:** nothing · **Blocks:** 020, 030, 060, 170

## Problem

`Message` (`cosmos-llm-rust/src/types.rs:20`) carries `content: String` plus
side fields for tool calls and tool results. A coding agent needs content that
does not fit in a string:

- **Images.** Pasted screenshots, image files read by a tool.
- **Thinking.** Anthropic requires the signed `thinking` block to be sent back
  unchanged on the assistant turn that preceded a `tool_result`. Today it is
  dropped at `providers/anthropic.rs:466` and cannot be replayed.
- **Cache markers.** Anthropic's `cache_control` attaches to a block, not to a
  message (see 030).
- **Mixed tool results.** A tool result can carry text and an image together
  (a `read` of a PNG, a screenshot tool).

Each provider currently rebuilds blocks from the side fields in
`messages_to_wire`. That works for text plus tool calls but has no place to put
anything else.

## Design

Make the message a list of blocks. Keep the constructors' signatures so most
callers do not notice.

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ContentBlock {
    Text { text: String },
    Image { source: ImageSource },
    Thinking { text: String, signature: Option<String> },
    /// Anthropic's `redacted_thinking`: opaque, replayed verbatim.
    RedactedThinking { data: String },
    ToolUse { id: String, name: String, input: Value },
    ToolResult { tool_use_id: String, content: Vec<ContentBlock>, is_error: bool },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImageSource {
    Base64 { media_type: String, data: String },
    Url { url: String },
}

pub struct Message {
    pub role: Role,                 // enum, see below
    pub content: Vec<ContentBlock>,
    pub cache: Option<CacheHint>,   // 030
}
```

`role` becomes `enum Role { System, User, Assistant, Tool }`. The string role
is a source of typos and costs nothing to fix while the crate is at 0.1.

Accessors replace field reads:

- `Message::text(&self) -> String`: concatenated `Text` blocks. Covers what
  most current `msg.content` reads want.
- `Message::tool_calls(&self) -> impl Iterator<Item = ToolCall>`
- `Message::tool_result_id(&self) -> Option<&str>`

`Message::user("hi")`, `Message::tool_result(id, s)`,
`Message::assistant_with_tools(text, calls)` keep their signatures and build
blocks internally. Add `Message::user_blocks(Vec<ContentBlock>)` and
`ContentBlock::image_base64(media_type, bytes)`.

### Serde compatibility

Sessions get persisted (200), and there may be serialized `Message`s around
already. Deserialization should accept the old shape, a string `content` with
`tool_calls`/`tool_call_id` side fields, through a `#[serde(from = ...)]`
shim. Serialization always writes the new shape.

### Provider mapping

| Block | Anthropic | OpenAI chat completions | OpenRouter |
|---|---|---|---|
| `Text` | `text` block | string, or `{"type":"text"}` part | same as OpenAI |
| `Image` | `image` block, base64 or url source | `image_url` part (data: URL for base64) | same as OpenAI |
| `Thinking` | `thinking` block with `signature` | **dropped** on replay | `reasoning` / `reasoning_details` (see 020) |
| `RedactedThinking` | `redacted_thinking` | dropped | dropped |
| `ToolUse` | `tool_use` block | `tool_calls[]` on the assistant message | same as OpenAI |
| `ToolResult` | `tool_result` block in a user turn | separate `role: tool` message, text only | same as OpenAI |

OpenAI `role: tool` messages accept text only. An image in a tool result has
to become a text placeholder in the tool message plus a follow-up user message
carrying the image. Codex and pi-mono both do this; see
`pi-mono/packages/ai/src/providers/` for the conversion.

The existing merge of parallel tool results into one Anthropic user turn
(`anthropic.rs:204`) carries over unchanged. It just reads blocks now.

### Response side

`CompletionResponse::to_assistant_message()` must keep every block the
provider returned, in order, including thinking and redacted thinking. That is
the whole point of the change. `CompletionResponse::text()` keeps returning
the concatenated text.

`StreamAccumulator` has to rebuild blocks in order too. Today it keeps one
`text: String`. It needs an ordered list of in-progress blocks keyed by the
provider's block index.

## Migration

1. Add `ContentBlock`, `ImageSource`, `Role`. Change `Message`.
2. Fix providers' `messages_to_wire` and response mapping.
3. Fix `StreamAccumulator`.
4. Fix call sites in `cosmos-llm-tool` (`agent.rs`), `cosmos-llm-predict`
   (`model.rs`), and `cosmos-llm-adapter` (`message.rs`). The adapter has its
   own message type; check whether it should now reuse this one.
5. Bump to 0.2 and add a CHANGELOG entry listing the field changes.

## Tests

- Round trip per provider: build a request with every block type and assert
  the wire JSON. Existing `wire_*` tests in `anthropic.rs` are the pattern.
- Response with `thinking` + `text` + two `tool_use` → `to_assistant_message()`
  → `messages_to_wire` yields identical blocks in identical order.
- Image in a tool result on OpenAI produces the text placeholder plus
  follow-up user message.
- Old-shape JSON deserializes.
- `StreamAccumulator` fed interleaved thinking/text/tool deltas produces the
  same message as the non-streaming response for the same fixture.

## References

- `pi-mono/packages/ai/src/types.ts`: a compact block model across providers.
- `codex/codex-rs/protocol/src/models.rs`: `ResponseItem` / `ContentItem`.
- `goose/crates/goose-provider-types`: `MessageContent`.

## Open questions

- Should `system` stay a message, or move to a `CompletionRequest::system:
  Vec<ContentBlock>` field? Anthropic wants it separate, and caching the
  system prompt is easier if it is its own field. Leaning towards the field,
  and keeping `Message::system` as a convenience that the client hoists.
