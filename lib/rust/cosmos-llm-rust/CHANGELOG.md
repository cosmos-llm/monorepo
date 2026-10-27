# Changelog

## Unreleased

### Breaking

- `Message` holds typed content. `content: String` is now
  `content: Vec<ContentBlock>`, `role: String` is now `role: Role`, and the
  `tool_call_id` and `tool_calls` fields are gone: a tool result is a
  `ToolResult` block and a requested call a `ToolUse` block. Use
  `Message::text()`, `tool_calls()` and `tool_result_id()` to read them back.
  The constructors (`user`, `assistant`, `tool_result`, `assistant_with_tools`,
  ...) keep their signatures; `Message::new` takes a `Role`. Deserialization
  still accepts the old shape (string `content` plus side fields);
  serialization writes the new one.
- `CompletionResponse::text()` and `content()` return owned `String`s
  (`Option<String>` for `content`), since a message can hold several text
  blocks. `StreamChunk` gained a `thinking` field, and `Usage`, `StreamChunk`
  and `CompletionRequest` gained fields, so struct literals need
  `..Default::default()` or the new constructors. `Usage` is
  `#[non_exhaustive]`.
- `Usage::prompt_tokens` counts every input token the model saw, cached ones
  included, for every provider. Anthropic reports its `input_tokens` without
  cache reads and writes; the provider now adds them back.
- `Provider` gained two defaulted methods, `models_info` and
  `lists_model_details`, so out-of-tree providers keep compiling.
  `CosmosError::Cancelled` is a new variant (the enum is `#[non_exhaustive]`).

### Added

- Content blocks: `ContentBlock` (`Text`, `Image`, `Thinking`,
  `RedactedThinking`, `ToolUse`, `ToolResult`), `ImageSource`, `Role`,
  `Message::user_blocks` / `assistant_blocks` / `tool_result_blocks` /
  `tool_error`, `ContentBlock::image_base64`. Providers lower them to wire
  JSON: images as `image` / `image_url` parts; on OpenAI an image in a tool
  result becomes a text note plus a follow-up user message. A response keeps
  every block in order, and `StreamAccumulator` rebuilds them in order
  (`StreamAccumulator::blocks`).
- Reasoning: `CompletionRequest::with_reasoning(Reasoning)` with
  `Reasoning::{Off, Effort, Budget}` and `ReasoningEffort`. Maps to Anthropic
  `thinking` (raising `max_tokens`, dropping `temperature`/`top_p`), OpenAI
  `reasoning_effort`, and OpenRouter `reasoning`. Streams arrive as
  `StreamChunk::thinking` (`ThinkingDelta::{Text, Signature, Redacted}`) and
  are kept as `Thinking` blocks so signed thinking is replayed on the next
  turn. `Usage::reasoning_tokens`. A model the built-in table knows lacks
  reasoning fails locally with `CosmosError::Configuration`.
- Prompt caching: `Message::with_cache`, `CompletionRequest::with_cache_tools`
  (`CacheHint::{Ephemeral, EphemeralLong}`) and
  `apply_default_cache_breakpoints`. Anthropic gets `cache_control` (at most
  four breakpoints, trimmed oldest first); OpenRouter forwards it; OpenAI
  never sees it. `Usage::cache_read_tokens` and `cache_write_tokens` are read
  from all three providers.
- Cancellation: `CompletionRequest::with_cancel(CancellationToken)`
  (re-exported as `cosmos_llm::CancellationToken`) and
  `CosmosError::Cancelled` / `is_cancelled()`. `Client` drops the in-flight
  request, never starts a retry attempt after a cancel, and ends a stream with
  one `Err(Cancelled)`.
- Model metadata: `ModelInfo` and `Pricing`; `Client::model_info`,
  `models_info`, `known_model_info` and `with_model_info` (overrides). Sources
  are overrides, the provider listing (OpenRouter's carries windows and prices),
  and a built-in table authored in `models.trongo` and compiled to JSON by
  `build.rs`. `ModelInfo::estimate_cost` prices a `Usage` from the table; it is
  separate from the charged `Usage::cost`.
- `tokens` module: `estimate_tokens`, `TokenEstimator`, `CharsEstimator`
  (`ceil(chars / 4)`, a flat cost per image and per message), for sizing the
  messages added since the last response's exact `prompt_tokens`.
- `trongo` feature (on by default): `from_trongo` / `to_trongo` for the IR
  types, and golden JSON + `.trongo` fixtures under `tests/golden/`.
- `MockProvider::respond_with_blocks`, and streamed mock replies now carry
  thinking deltas.

- `Client::provider_name`, the canonical name of the provider a client
  talks to.
- Initial release: unified Rust client for OpenAI and Anthropic.
- `Client` with `complete`, `completion`, `chat`, and `models` methods.
- `Config` with env-variable loading (`CLLM__<PROVIDER>__<SETTING>`).
- `Provider` trait for adding new backends.
- OpenAI provider: chat completions, tool calling, model listing.
- Anthropic provider: chat completions with system message support, static model list.
- `CompletionRequest` builder API (`with_temperature`, `with_max_tokens`, etc.).
- OpenRouter provider: chat completions, tool calling, model listing, and
  optional `HTTP-Referer` / `X-Title` attribution headers.
- `opencode` module and `Config::load_opencode_auth` / `load_opencode_auth_from`
  for reusing API keys from the opencode CLI's `auth.json`. Opt-in; existing
  keys are never overwritten.
- devenv.nix + cargo-zigbuild static build support per RFC_RUST_DEVENV_ZIG_STATIC_BUILDS.
- Streaming completions for all three providers, via
  `Provider::stream_completion` and `Client::stream` /
  `Client::stream_completion` / `Client::stream_to_completion`.
- `sse` module with `SseDecoder`, an incremental server-sent-events parser that
  buffers across chunk boundaries so an event split over several packets is
  still decoded as one unit.
- `StreamChunk` now carries `tool_calls` and `usage` alongside `delta` and
  `finish_reason`; `ToolCallDelta` represents a partial tool call.
- `StreamAccumulator` folds a chunk sequence back into a `CompletionResponse`,
  reassembling tool-call arguments from partial JSON, so a tool-calling loop
  works identically with streaming and non-streaming calls.
- `examples/streaming.rs`.
- Configurable API roots: `with_base_url` / `base_url` on all three providers,
  `Client::new_with_base_url`, `Client::with_provider_at`, and
  `providers::resolve_with_base_url`. Each provider also reads a `*_BASE_URL`
  environment variable (`OPENAI_BASE_URL` / `CLLM__OPENAI__BASE_URL`, and so
  on), with an explicit builder call taking precedence. This makes
  OpenAI-compatible servers (vLLM, llama.cpp, Azure OpenAI) and proxies usable
  through the existing providers.
- `tests/streaming.rs`: end-to-end streaming tests against a mockito server,
  covering text, tool-call reassembly, usage, pre-stream error statuses, and
  mid-stream error payloads for each provider.

### Notes

- `Provider::supports_streaming` reports what a provider actually implements,
  not what its upstream API offers. All three built-in providers now implement
  streaming, so `Client::can_stream` returns `true` for each. The
  `Provider::stream_completion` default still returns `CosmosError::Streaming`,
  so third-party backends stay source-compatible.
- OpenAI and OpenRouter requests set `stream_options.include_usage`, since those
  APIs otherwise omit token counts from streamed responses. Anthropic reports
  input tokens on `message_start` and output tokens on `message_delta`; the
  provider combines them into one `Usage` on the final chunk.
