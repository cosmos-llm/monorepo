# Cosmos LLM — Rust Libraries

Four crates with a layered architecture. `cosmos-llm-virtual-filesystem` is the base; everything else builds on it.

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

Function-calling layer for LLM agents. Optional `webfetch` feature adds web content retrieval.

```toml
cosmos-llm-tool = { version = "0.1", features = ["webfetch"] }
```

### `cosmos-llm-virtual-filesystem` — [cosmos-llm-rust-virtual-filesystem](cosmos-llm-rust-virtual-filesystem/)

Minimal in-memory virtual filesystem with `serde` serialization. No async, no heavy deps — just `serde` and `thiserror`.

## Dependency graph

```
cosmos-llm-virtual-filesystem
        ↑
cosmos-llm-context
        ↑              ↑
cosmos-llm-tool    cosmos-llm (client)
```

## Development

Requires Rust 1.75+.

```sh
cd cosmos-llm-rust
cargo test
cargo test --features webfetch  # for the tool crate
```

Release builds use LTO, single codegen unit, and stripped binaries.

## License

MIT. Enterprise support from [Durable Programming](https://durableprogramming.com).
