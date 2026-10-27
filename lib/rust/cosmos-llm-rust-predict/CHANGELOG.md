# Changelog

## Unreleased

### Added

- Initial release, porting the predictors from the Ruby `cosmos-llm-predict`
  gem.
- `Predict`: coerce inputs, render through an adapter, call a model, parse,
  and retry unparseable replies with a correction turn.
- `Predict::chain_of_thought`, adding a leading `reasoning` output.
- `BestOfN`, sampling any `Module` with a reward function, an optional
  early-stop threshold, and a per-draw cache salt.
- `Module` trait, with layered `CallOptions` passed down to nested modules.
- `LanguageModel` trait, implemented for `cosmos_llm::Client` under the
  default `client` feature.
- `BackendPlugin` (built-in `cosmos`) and `StrategyPlugin` (built-ins
  `predict`, `chain_of_thought`) registries.
- Reply caching through any `cosmos_llm_cache::CacheStore`.
- `Prediction` with typed `deserialize`, usage, cache status, and attempts.
- `Predict::dump_state` / `load_state` for instructions and demonstrations.
- `cosmos-llm-predict` CLI behind the `cli` feature: `run` (single or
  `--jsonl`, with ordered concurrency) and `plugins`.
