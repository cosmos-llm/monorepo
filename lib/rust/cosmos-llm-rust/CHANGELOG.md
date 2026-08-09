# Changelog

## Unreleased

### Added

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
