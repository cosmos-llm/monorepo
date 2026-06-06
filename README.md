# Cosmos LLM

A monorepo of libraries for integrating Large Language Models across Ruby, Rust, and JavaScript.

Each language has a parallel set of libraries that share a common architecture: a unified client, a context DSL, a tool/function-calling layer, and a virtual filesystem abstraction.

## Libraries

### Ruby (`lib/ruby/`)

| Gem | Purpose |
|-----|---------|
| `cosmos-llm` | Unified client for 15+ LLM providers (OpenAI, Anthropic, Google, Cohere, Mistral, Groq, etc.) |
| `cosmos-llm-context` | DSL for building structured agentic contexts with virtual filesystem support |
| `cosmos-llm-tool` | Tool registration and execution for LLM function calling |
| `cosmos-llm-tool-preset` | Ready-to-use tools: file read/write, grep, web fetch |
| `cosmos-llm-virtual-filesystem` | Hierarchical in-memory filesystem for LLM context sandboxing |

### Rust (`lib/rust/`)

| Crate | Purpose |
|-------|---------|
| `cosmos-llm` | Unified async client for OpenAI and Anthropic (completions, streaming) |
| `cosmos-llm-context` | DSL for composing agentic LLM contexts |
| `cosmos-llm-tool` | Function-calling layer with optional web fetch feature |
| `cosmos-llm-virtual-filesystem` | Minimal in-memory virtual filesystem with serde support |

### JavaScript (`lib/js/`)

| Package | Purpose |
|---------|---------|
| `cosmos-llm` | TypeScript/JavaScript client for OpenAI and Anthropic |

## Architecture

The stack is layered. `virtual-filesystem` is the base — it has no upstream dependencies within this repo. `context` builds on it. `tool` and `tool-preset` build on `client` and `context`. This keeps each layer independently testable and usable.

## Applications

See `apps/` for demo applications built on these libraries.

## License

MIT. Enterprise support available from [Durable Programming](https://durableprogramming.com).
