# Cosmos LLM — Rust Libraries

Small crates with a layered architecture. The client side builds on `cosmos-llm-virtual-filesystem`; the signature-driven side (signatures, adapters, predictors, evaluation) builds on `cosmos-llm-registry`. Every crate is a library first, and the ones with a natural command line ship it behind a `cli` feature.

## Crates

### `cosmos-llm` — [cosmos-llm-rust](cosmos-llm-rust/)

Async client for OpenAI and Anthropic.

- Chat completions and streaming via `reqwest`
- Model discovery
- `tokio` runtime, `rustls` TLS, no OpenSSL dependency

```rust
use cosmos_llm::Client;

let client = Client::anthropic(api_key);
let response = client.complete(messages).await?;
```

### `cosmos-llm-context` — [cosmos-llm-rust-context](cosmos-llm-rust-context/)

DSL for composing structured agentic contexts. Integrates with the virtual filesystem for sandboxed file content.

### `cosmos-llm-tool` — [cosmos-llm-rust-tool](cosmos-llm-rust-tool/)

Function-calling layer for LLM agents. Optional `webfetch` feature adds web content retrieval. The `agent` feature adds `AgentLoop`: it streams the model, reports owned `AgentEvent`s over a bounded channel, keeps a `Thread` across runs (queued input is delivered between tool results and the next model call), asks an `Approver` before each tool call, and stops cleanly on a `CancellationToken` with a history every provider accepts. `ToolSpec` is the data-only half of a `ToolDefinition`.

```toml
cosmos-llm-tool = { version = "0.1", features = ["webfetch"] }
```

### `cosmos-llm-virtual-filesystem` — [cosmos-llm-rust-virtual-filesystem](cosmos-llm-rust-virtual-filesystem/)

Minimal in-memory virtual filesystem with `serde` serialization. No async, no heavy deps — just `serde` and `thiserror`.

### `retry-policy` — [retry-policy](retry-policy/)

Retry schedules with exponential backoff, jitter, and attempt records. Used by the client's opt-in `Client::with_retry`.

## Signature-driven crates

Ports of the Ruby `signature`, `predict`, and `evaluate` gems, split along their seams. Each owns one trait, and every implementation reachable by name is registered with [`inventory`](https://docs.rs/inventory). Registering under a built-in's name replaces it, so any piece can be swapped without forking the crate that owns it. These are standalone pieces; an orchestrator that wires them together is future work.

| Crate | Owns | Built-ins | CLI |
|---|---|---|---|
| [`cosmos-llm-registry`](cosmos-llm-rust-registry/) | `Plugin`, name resolution, overrides | — | — |
| [`cosmos-llm-signature`](cosmos-llm-rust-signature/) | `Signature`, `FieldCodec` | 6 JSON types | `show`, `schema`, `coerce` |
| [`cosmos-llm-example`](cosmos-llm-rust-example/) | `Example`, `DevsetFormat` | `jsonl`, `json` | `inspect`, `inputs`, `labels`, `split`, `convert` |
| [`cosmos-llm-adapter`](cosmos-llm-rust-adapter/) | `Adapter` | `chat`, `json`, `json_schema` | `format`, `parse` |
| [`cosmos-llm-cache`](cosmos-llm-rust-cache/) | `CacheStore` | `memory`, `disk`, `tiered` | `key`, `get`, `put`, `remove`, `clear`, `stats` |
| [`cosmos-llm-predict`](cosmos-llm-rust-predict/) | `LanguageModel`, `Module` | `cosmos` backend; `predict`, `chain_of_thought` | `run` |
| [`cosmos-llm-evaluate`](cosmos-llm-rust-evaluate/) | `Metric`, `Program` | `exact_match`, `f1`, `contains`, `set_overlap`, `grounded` | `score`, `evidence` |

Every CLI also has `plugins`, which lists registrations, including shadowed ones. The CLIs compose over JSON Lines:

```sh
cosmos-llm-example inputs dev.jsonl -i question \
  | cosmos-llm-predict run 'question -> answer' --jsonl --provider openai --model gpt-5 --cache-dir .cache \
  > preds.jsonl
cosmos-llm-evaluate score dev.jsonl preds.jsonl --metric exact_match --fail-under 0.9
```

To get your own plugins into a CLI, link your plugin crate into a three-line binary that calls the library's `cli::main()`. See the [registry README](cosmos-llm-rust-registry/README.md).

## Dependency graph

```
cosmos-llm-virtual-filesystem
        ↑
cosmos-llm-context
        ↑              ↑
cosmos-llm-tool    cosmos-llm (client) ── retry-policy
```

```
cosmos-llm-registry                    (every crate below depends on it)
cosmos-llm-signature
cosmos-llm-example
cosmos-llm-cache
cosmos-llm-adapter    → signature
cosmos-llm-evaluate   → example
cosmos-llm-predict    → signature, example, adapter, cache, cosmos-llm (optional)
```

## Development

Requires Rust 1.75+.

```sh
cd cosmos-llm-rust
cargo test
cargo test --features webfetch  # for the tool crate

cd ../cosmos-llm-rust-predict
cargo test --all-features       # the signature-driven crates each test this way
```

Release builds use LTO, single codegen unit, and stripped binaries.

## License

MIT. Enterprise support from [Durable Programming](https://durableprogramming.com).
