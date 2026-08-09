# cosmos-llm

A unified Rust client for multiple LLM providers. Part of the [Cosmos-LLM](https://github.com/cosmos-llm) monorepo.

## Supported providers

| Name | Completion | Tools | Streaming | Models |
|---|---|---|---|---|
| `openai` | ✓ | ✓ | ✓ | ✓ |
| `anthropic` | ✓ | ✓ | ✓ | ✓ (static list) |
| `openrouter` | ✓ | ✓ | ✓ | ✓ |

`Client::can_stream()` reports whether the active provider implements
streaming.

## Installation

```toml
[dependencies]
cosmos-llm = "0.1"
```

## Usage

```rust
use cosmos_llm::{Client, Message, CompletionRequest};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // API key from env: OPENAI_API_KEY or CLLM__OPENAI__API_KEY
    let client = Client::new("openai", std::env::var("OPENAI_API_KEY")?)?
        .with_model("gpt-4o");

    // One-shot completion
    let text = client.complete("What is the capital of France?").await?;
    println!("{text}");

    // Full chat with system message
    let req = CompletionRequest::new(
        "gpt-4o",
        vec![
            Message::system("You are a concise assistant."),
            Message::user("Name three planets."),
        ],
    )
    .with_temperature(0.5)
    .with_max_tokens(256);

    let resp = client.chat(req).await?;
    println!("{}", resp.content().unwrap_or(""));

    Ok(())
}
```

## Streaming

`Client::stream` returns a `Stream` of `StreamChunk`s as the model generates
them:

```rust
use cosmos_llm::Client;
use futures_util::StreamExt;
use std::io::Write;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::new("openai", std::env::var("OPENAI_API_KEY")?)?
        .with_model("gpt-4o");

    let mut stream = client.stream("Write a haiku about Rust").await?;
    while let Some(chunk) = stream.next().await {
        print!("{}", chunk?.delta);
        std::io::stdout().flush()?;
    }

    Ok(())
}
```

Use `Client::stream_completion` for full control over the request, and
`StreamAccumulator` to fold the chunks back into a `CompletionResponse` — the
same shape a non-streaming call returns, complete with assembled tool calls and
token usage:

```rust
use cosmos_llm::{Client, CompletionRequest, Message, StreamAccumulator};
use futures_util::StreamExt;

async fn run(client: Client) -> Result<(), Box<dyn std::error::Error>> {
    let req = CompletionRequest::new("gpt-4o", vec![Message::user("Hi")]);
    let mut stream = client.stream_completion(req).await?;
    let mut acc = StreamAccumulator::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        print!("{}", chunk.delta);
        acc.push(&chunk);
    }

    let response = acc.into_response();
    println!("{:?}", response.usage);
    Ok(())
}
```

`Client::stream_to_completion` wraps that loop: it hands each chunk to a
callback for live output and returns the assembled response.

Streamed tool calls arrive as `ToolCallDelta` fragments, since providers send
tool arguments as partial JSON text. The accumulator reassembles them, so a
tool-calling loop written against `CompletionResponse` works unchanged with
either mode.

See `examples/streaming.rs` for a runnable version:

```
OPENAI_API_KEY=sk-... cargo run --example streaming
```

## Configuration

### Programmatic

```rust
use cosmos_llm::{Client, Config};

let mut config = Config::new();
config.set_api_key("anthropic", "sk-ant-...");
config.set_model("anthropic", "claude-3-5-sonnet-20241022");
config.set_default_provider("anthropic");

let client = Client::from_config(config, "anthropic")?;
```

### Environment variables

```bash
export CLLM__OPENAI__API_KEY=sk-...
export CLLM__ANTHROPIC__API_KEY=sk-ant-...
export CLLM__OPENROUTER__API_KEY=sk-or-...
export CLLM__OPENAI__MODEL=gpt-4o
```

Each provider also accepts its conventional variable — `OPENAI_API_KEY`,
`ANTHROPIC_API_KEY`, `OPENROUTER_API_KEY`.

### Custom endpoints

Every provider's API root can be overridden, either per-client or by
environment variable. The provider name still selects the wire format, so any
OpenAI-compatible server works through the `openai` provider:

```rust
use cosmos_llm::Client;

// A local vLLM or llama.cpp server speaking the OpenAI protocol.
let client = Client::new_with_base_url(
    "openai",
    "not-checked-locally",
    "http://localhost:8000/v1",
)?
.with_model("meta-llama/Llama-3-8b");
```

```bash
export OPENAI_BASE_URL=http://localhost:8000/v1
# or: export CLLM__OPENAI__BASE_URL=http://localhost:8000/v1
```

An explicit `with_base_url` call takes precedence over the environment. At the
provider level, `OpenAiProvider::with_base_url` (and the equivalent on the
other two) does the same thing, and `Client::with_provider_at` switches an
existing client to a provider at a custom root.

### Reusing opencode credentials

If you already authenticated providers with `opencode auth login`, those keys
can be loaded from `~/.local/share/opencode/auth.json` instead of being
re-exported:

```rust
use cosmos_llm::{Client, Config};

let mut config = Config::new();
let loaded = config.load_opencode_auth()?;   // -> ["openrouter"]

let client = Client::from_config(config, "openrouter")?
    .with_model("anthropic/claude-3.5-sonnet");
```

This is opt-in; `Config::new()` never reads the file. Keys already set — from
the environment or `set_api_key` — take precedence and are not overwritten.
Only `api`-type entries are used; opencode's OAuth credentials are skipped
because it refreshes those itself. A missing file is not an error. Use
`load_opencode_auth_from(path)` to read a specific file.

## OpenRouter

OpenRouter namespaces models by vendor and exposes an OpenAI-compatible API:

```rust
use cosmos_llm::Client;

let client = Client::new("openrouter", std::env::var("OPENROUTER_API_KEY")?)?
    .with_model("anthropic/claude-3.5-sonnet");

let text = client.complete("What is the capital of France?").await?;
```

To appear on OpenRouter's public leaderboards, set the optional attribution
headers via the provider builder or the `OPENROUTER_REFERER` / `OPENROUTER_TITLE`
environment variables:

```rust
use cosmos_llm::providers::openrouter::OpenRouterProvider;

let provider = OpenRouterProvider::new(None)
    .with_referer("https://example.com")
    .with_title("my-app");
```

## Development Setup

This project uses [devenv](https://devenv.sh/) for reproducible environments.

### Prerequisites

- [Nix](https://nixos.org/download.html)
- [devenv](https://devenv.sh/getting-started/)

### Getting started

```bash
git clone <repo>
cd cosmos-llm-rust
devenv shell

cargo test
cargo doc --no-deps --open
```

### Building a static release binary

```bash
cargo br   # alias for: cargo zigbuild --release --target x86_64-unknown-linux-musl
```

## License

MIT
