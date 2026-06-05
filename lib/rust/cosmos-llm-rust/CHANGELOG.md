# Changelog

## Unreleased

### Added

- Initial release: unified Rust client for OpenAI and Anthropic.
- `Client` with `complete`, `completion`, `chat`, and `models` methods.
- `Config` with env-variable loading (`CLLM__<PROVIDER>__<SETTING>`).
- `Provider` trait for adding new backends.
- OpenAI provider: chat completions, embeddings, model listing, streaming flag.
- Anthropic provider: chat completions with system message support, static model list.
- `CompletionRequest` builder API (`with_temperature`, `with_max_tokens`, etc.).
- devenv.nix + cargo-zigbuild static build support per RFC_RUST_DEVENV_ZIG_STATIC_BUILDS.
