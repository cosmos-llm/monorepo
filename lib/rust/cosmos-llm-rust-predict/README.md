# cosmos-llm-predict

Signature-driven LLM calls. A Rust port of the predictor half of the Ruby
`cosmos-llm-predict` gem.

A `Predict` takes a [signature](../cosmos-llm-rust-signature/) and makes one
model call that satisfies it. The [adapter](../cosmos-llm-rust-adapter/)
renders the prompt, a `LanguageModel` answers it, and the adapter parses the
reply into typed values.

```rust
use std::sync::Arc;
use cosmos_llm::Client;
use cosmos_llm_predict::Predict;
use cosmos_llm_signature::Signature;
use serde_json::json;

let model = Client::new("anthropic", std::env::var("ANTHROPIC_API_KEY")?)?
    .with_model("claude-sonnet-5-5");

let triage = Predict::new(Signature::parse("ticket -> severity: int, team")?, Arc::new(model));
let prediction = triage.call(&json!({ "ticket": "Checkout returns 500 for every user" })).await?;

prediction.values["severity"]; // already an integer
```

## What a call does

1. **Inputs are coerced** against the signature, so a missing field fails
   before any money is spent.
2. **The adapter renders** the system prompt, any demonstrations as prior turns,
   and the inputs.
3. **Adapter request parameters** (such as `json_schema`'s `response_format`)
   fill gaps in the call options. An option you set by name wins.
4. **The model answers,** through the cache when one is set.
5. **The reply is parsed.** If parsing fails, the model is shown its own reply and
   the parse error and asked again: once by default, `max_parse_retries(n)` to
   change it. Models usually fix a format mistake once shown it, and one extra
   call is cheaper than a failed run.

A `Prediction` keeps the coerced `values` apart from the bookkeeping:
`completion`, `usage`, `cached`, `attempts`, and `meta`. A metric comparing
outputs to labels never trips over it. `prediction.deserialize::<T>()` gives
you your own struct.

`usage` sums only the attempts that reached the model. A cache hit cost nothing,
so a fully cached prediction reports `None`, which means "nothing spent", not
"unknown".

## Strategies

- **`Predict::new`:** one call.
- **`Predict::chain_of_thought`:** adds a `reasoning` output ahead of the
  declared outputs. Adapters render outputs in order, so the model reasons
  first and answers after. Field ordering is the whole mechanism.
- **`BestOfN::new(module, n, reward)`:** samples any module up to `n` times at a
  raised temperature and keeps the highest-scoring draw. `threshold(x)` stops
  at the first draw scoring at least `x`.
  - Failed draws are skipped.
  - Each draw gets its own cache salt. In the Ruby gem, a cache would replay the
    first draw N times.
  - The winner's `usage` is the total across all draws, since that is what the
    answer cost.

Anything implementing `Module` (inputs and `CallOptions` in, a `Prediction`
out) nests inside the others. `CallOptions` passed to an outer module are
layered over each inner module's defaults, which is how `BestOfN` raises the
temperature of a predictor it does not own.

## Swapping pieces

Every collaborator is a trait object. In code, pass your implementation to the
builder. By name, from the CLI or config, register it through the owning
crate's re-exported `registry`. A registration under a built-in's name replaces
the built-in.

| Piece | Trait | Registry | Built-ins |
|---|---|---|---|
| Model | `LanguageModel` | `BackendPlugin` | `cosmos` (the cosmos-llm `Client`) |
| Prompt format | `Adapter` | `AdapterPlugin` | `chat`, `json`, `json_schema` |
| Reply cache | `CacheStore` | `CachePlugin` | `memory`, `disk`, `tiered` |
| Strategy | `Module` | `StrategyPlugin` | `predict`, `chain_of_thought` (`cot`) |

`LanguageModel` is two methods, `complete(messages, options)` and `identity()`.
That is enough to put a local model, a recorded fixture, or another process
behind a predictor. With `default-features = false` this crate does not depend
on `cosmos-llm` at all.

```rust
use cosmos_llm_predict::{BackendPlugin, registry::meta};

cosmos_llm_predict::registry::inventory::submit! {
    BackendPlugin { meta: meta!("llama_cpp").summary("Local llama.cpp server"), build: build_llama }
}
```

Then `cosmos-llm-predict run ... --backend llama_cpp` works in any binary that
links your crate. See "Linking a plugin crate" in the
[registry README](../cosmos-llm-rust-registry/README.md).

## Command line

```sh
cargo install --path . --features cli

cosmos-llm-predict run 'question -> answer' '{"question": "Why is the sky blue?"}' \
    --provider anthropic --model claude-sonnet-5-5

# One prediction per input line, in order, four at a time, cached.
cosmos-llm-example inputs dev.jsonl -i question \
  | cosmos-llm-predict run 'question -> answer: int' --jsonl --concurrency 4 \
      --strategy cot --provider openai --model gpt-5 --cache-dir ~/.cache/cosmos-llm \
  > preds.jsonl

cosmos-llm-predict plugins
```

API keys are read from the environment the way `cosmos-llm` reads them. In
`--jsonl` mode a row that fails prints `{}`, so the output stays aligned with the
input. The error goes to stderr with its line number, and the command exits 1
after finishing the remaining rows. `--full` prints the raw reply, usage, and
cache status along with the values. `--base-url` points the backend at a proxy
or an OpenAI-compatible server.

## Not yet ported

The Ruby gem's process-wide `Settings` is deliberately absent: collaborators
are passed explicitly, so tests and optimizers never fight over global state.
Ruby's `Module#named_predictors`, which reflects over instance variables to find
every predictor in a program, has no direct Rust equivalent. It will arrive with
the optimizer that needs it. `dump_state`/`load_state` on `Predict` are already
here for saving tuned instructions and demonstrations.

## Requirements

Rust 1.75 or later. Features: `client` (default) implements `LanguageModel` for
`cosmos_llm::Client` and registers the `cosmos` backend; `cli` builds the binary.

## License

MIT. Enterprise support from [Durable Programming](https://durableprogramming.com).
