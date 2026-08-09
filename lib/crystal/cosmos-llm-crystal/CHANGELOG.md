# Changelog

## Unreleased

### Added

- Initial release: unified Crystal client for OpenAI and Anthropic.
- `CosmosLLM::Client` with `complete`, `completion`, `chat`, and `models` methods.
- `CosmosLLM::Config` with env-variable loading (`CLLM__<PROVIDER>__<SETTING>`).
- `CosmosLLM::Providers::Base` abstract class for adding new backends.
- OpenAI provider: chat completions, tool calling, model listing, streaming flag.
- Anthropic provider: chat completions with system message support, tool calling,
  static model list.
- `CompletionRequest` chainable builder API (`with_temperature`, `with_max_tokens`, etc.).
- devenv.nix with `languages.crystal.enable`.
