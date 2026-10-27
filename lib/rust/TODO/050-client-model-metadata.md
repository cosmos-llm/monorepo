# 050 — Model metadata

**Status:** implemented — `ModelInfo`/`Pricing` in `model.rs`, `Client::model_info`/`models_info`/`known_model_info`/`with_model_info`, OpenRouter catalogue parsing, built-in table authored in `models.trongo` and compiled by `build.rs`, `ModelInfo::estimate_cost`. Deferred: the core-config override loader (belongs to `cosmos-llm-agent-core`); table prices are only filled in where confident and need a review pass.

**Crate:** `cosmos-llm` · **Depends on:** nothing · **Blocks:** 160, 170, 020 validation

## Problem

`Client::models()` returns `Vec<String>`. A coding agent needs to know, for
the model it is using:

- context window (input) and max output tokens: compaction triggers and the
  status bar's "context %" both depend on it,
- whether it supports tools, images, reasoning, prompt caching,
- prices, to estimate cost when the provider reports none (Anthropic and
  OpenAI do not return cost; OpenRouter does).

## Design

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ModelInfo {
    pub id: String,
    pub provider: String,
    pub context_window: Option<u32>,
    pub max_output_tokens: Option<u32>,
    pub supports_tools: bool,
    pub supports_images: bool,
    pub supports_reasoning: bool,
    pub supports_prompt_cache: bool,
    pub pricing: Option<Pricing>, // per million tokens: input, output, cache_read, cache_write
}

impl Client {
    pub async fn model_info(&self, id: &str) -> Result<ModelInfo, CosmosError>;
    pub async fn models_info(&self) -> Result<Vec<ModelInfo>, CosmosError>;
}
```

### Sources, in order of preference

1. **Provider API.** OpenRouter's `/models` has `context_length`, pricing and
   `supported_parameters`. Anthropic's `/v1/models` has ids and display names
   only. OpenAI's `/models` has ids only.
2. **Built-in table.** A TOML file compiled in with `include_str!`, covering
   the current Anthropic and OpenAI models. Codex ships a similar list in
   `codex-rs/models-manager`; crush pulls from Catwalk.
3. **User override.** A config entry for local and proxy models, in the core
   config (005):

   ```clojure
   {:models {"my-local-model" {:context_window 32768
                               :supports_tools true}}}
   ```

   The core evaluates it and hands `cosmos-llm` plain `ModelInfo` values.

The built-in table (2) is authored as `models.trongo`, so families of models
can share defaults through a function. A `build.rs` step evaluates it with
`trongoscript` (a build dependency) and embeds the result as JSON, so loading
the table at runtime needs no evaluator, even with the `trongo` feature off.

Unknown models get `None` limits. Callers then fall back to a conservative
default (e.g. 128k) and say so, rather than guessing silently.

### Cost estimate

`Usage::cost` stays what the provider charged. Add
`ModelInfo::estimate_cost(&Usage) -> Option<f64>` for providers that report
nothing. The two must never be mixed into one number without a label.

## Tests

- OpenRouter `/models` fixture → `ModelInfo` with window and pricing.
- Built-in table has an entry for every model named in provider docs/tests.
- Override in config wins over the table.
- `estimate_cost` with cache read/write tokens uses the cache prices.

## References

- `codex/codex-rs/models-manager`, `codex/codex-rs/model-provider-info`.
- `crush/internal/config`: provider/model catalog.
- `pi-mono/packages/ai/src/models.generated.ts`: generated table.
