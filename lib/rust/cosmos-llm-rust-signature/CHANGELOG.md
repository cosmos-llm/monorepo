# Changelog

## Unreleased

### Added

- Initial release, porting the Ruby `cosmos-llm-signature` gem.
- `Signature` with shorthand parsing (`question -> answer: int`), a builder,
  validated serde round-tripping, and `from_spec` accepting shorthand,
  `@file.json`, or a registered name.
- `Field` with descriptions, enums (`one_of`), optional fields, and defaults.
- `output_json_schema`, `coerce_inputs`, `coerce_outputs`, `cache_key`,
  `with_instructions`, and `with_leading_output`.
- Field types as swappable `FieldCodec`s in the `CodecPlugin` registry, with
  built-ins `string`, `integer`, `number`, `boolean`, `array`, and `object`.
- `SignaturePlugin` registry for named signatures.
- `cosmos-llm-signature` CLI behind the `cli` feature: `show`, `schema`,
  `coerce`, `plugins`.

### Changed (relative to the Ruby gem)

- Integer fields reject fractional floats instead of truncating them.
- Number fields reject NaN and infinity.
- A `null` value for a required field without a default is a missing-field
  error rather than a silent `nil`.
