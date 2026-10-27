# Changelog

## Unreleased

### Added

- Initial release, porting `Example` from the Ruby `cosmos-llm-predict` gem
  into its own crate.
- `Example` with input keys, `inputs`, `labels`, `missing_inputs`, `merge`,
  and `without`; serializes as its plain value object.
- `devset` module: `read`, `load`, `write`, and a seeded, reproducible
  `split`.
- Devset formats as `DevsetFormat`s in the `FormatPlugin` registry, with
  built-ins `jsonl` and `json` chosen by file extension.
- `cosmos-llm-example` CLI behind the `cli` feature: `inspect`, `inputs`,
  `labels`, `split`, `convert`, `plugins`.
