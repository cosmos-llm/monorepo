# cosmos-llm

A unified Rust client for multiple LLM providers. Part of the [Cosmos-LLM](https://github.com/cosmos-llm) monorepo.

## Supported providers

| Name | Completion | Tools | Streaming | Models |
|---|---|---|---|---|
| `openai` | ✓ | ✓ | — | ✓ |
| `anthropic` | ✓ | ✓ | — | ✓ (static list) |
| `openrouter` | ✓ | ✓ | — | ✓ |

Streaming is not implemented yet. `Client::can_stream()` reports `false` for
every provider, and will start reporting `true` per-provider as streaming
lands.

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
