# Changelog

## Unreleased

### Added

- Initial release: shared resolution rules for the `inventory`-backed plugin
  registries in the cosmos-llm Rust crates.
- `Meta` with `const` builders and the `meta!` macro, which records the
  registering module as the origin.
- `Plugin` trait; `resolve`, `require`, `active`, `names`, `listing`, and
  `conflicts` over any collected registration type.
- `priority::BUILTIN` and `priority::DEFAULT`, so a same-named registration
  replaces a built-in without configuration.
- Case- and separator-insensitive name matching (`normalize`).
- `NotFound`, listing the registered alternatives.
- `inventory` re-exported for plugin crates.
