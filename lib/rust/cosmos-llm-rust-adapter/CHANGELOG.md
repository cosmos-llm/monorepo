# Changelog

## Unreleased

### Added

- Initial release, porting the adapters from the Ruby `cosmos-llm-predict` gem.
- `Adapter` trait: `format` and `parse` with defaults built from
  `format_instructions`, `render_inputs`, `render_demo_outputs`, and `extract`;
  `request_params` for extra request-body fields.
- `ChatAdapter`, laying fields out under `[[ ## name ## ]]` headers.
- `JsonAdapter`, asking for a single JSON object and finding it bare, fenced,
  or inside prose; `JsonAdapter::native` also sends the output schema as
  `response_format`.
- `AdapterPlugin` registry with built-ins `chat`, `json`, and `json_schema`;
  `adapter(name)` builds one by name.
- `AdapterError::Parse` carrying the failed reply, for correction turns.
- `json_stream` module for replies holding several JSON objects.
- `Message`, a minimal role/content message serializing like the client
  crate's.
- `cosmos-llm-adapter` CLI (`cli` feature) with `format`, `parse`, and
  `plugins` subcommands.
