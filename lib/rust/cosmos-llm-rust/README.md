# cosmos-llm

A unified Rust client for multiple LLM providers. Part of the [Cosmos-LLM](https://github.com/cosmos-llm) monorepo.

## Supported providers

| Name | Completion | Streaming | Models |
|---|---|---|---|
| `openai` | ✓ | ✓ | ✓ |
| `anthropic` | ✓ | — | ✓ (static list) |

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
export CLLM__OPENAI__MODEL=gpt-4o
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
