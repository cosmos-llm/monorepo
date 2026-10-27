# Changelog

## Unreleased

### Added

- `PromptPlan` IR (`PromptSection`, `SectionKind`, `Stability`, `CacheHint`,
  `PromptBlock`): the system prompt as typed, ordered sections, rendered to
  text last. `PromptPlan::assemble` builds the default order (base, tool
  guidance, instructions, environment) with the cache breakpoint after the
  instructions.
- Instruction loader: `load_instructions` reads the user-global
  `AGENTS.md` and each `AGENTS.md` / `CLAUDE.md` (plus `*.local.md`) from the
  git root down to the cwd, with `@path` imports (depth 5, cycle-safe,
  confined to the repo and config dir) and per-file / total size caps.
  `load_dir_instructions` loads one directory for lazy injection.
- Environment block: `probe_environment` gathers cwd, git state, platform and
  date into `EnvironmentFacts`; `EnvironmentFacts::render` renders it.
- `trongo` feature (default on): `from_trongo` / `to_trongo` for these IRs,
  with JSON and `.trongo` golden files under `tests/golden/`.
