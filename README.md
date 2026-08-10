# Cosmos LLM

A monorepo of libraries for integrating Large Language Models across Ruby, Rust, JavaScript, and Crystal.

Each language has a parallel set of libraries that share a common architecture: a unified client, a context DSL, a tool/function-calling layer, and a virtual filesystem abstraction.

The Ruby set is the most developed, followed by the Rust crates, then the JS and Crystal clients.

## Libraries

### Ruby (`lib/ruby/`)

| Gem | Purpose |
|-----|---------|
| `cosmos-llm` | Unified client for 15+ LLM providers (OpenAI, Anthropic, Google, Cohere, Mistral, Groq, etc.) |
| `cosmos-llm-context` | DSL for building structured agentic contexts with virtual filesystem support |
| `cosmos-llm-tool` | Tool registration and execution for LLM function calling |
| `cosmos-llm-tool-preset` | Ready-to-use tools: file read/write, grep, web fetch |
| `cosmos-llm-virtual-filesystem` | Hierarchical in-memory filesystem for LLM context sandboxing |
| `cosmos-llm-signature` | Declarative typed input/output signatures for a single LLM call |
| `cosmos-llm-predict` | Runs signatures against models: adapters, few-shot demos, caching, composable modules |
| `cosmos-llm-evaluate` | Scores modules against labeled devsets with parallel execution and built-in metrics |

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

### Crystal (`lib/crystal/`)

| Shard | Purpose |
|-------|---------|
| `cosmos-llm` | Client for OpenAI and Anthropic with normalized tool calling; stdlib-only, no third-party dependencies |

## Architecture

The stack is layered. `virtual-filesystem` is the base — it has no upstream dependencies within this repo. `context` builds on it. `tool` and `tool-preset` build on `client` and `context`. This keeps each layer independently testable and usable.

The Ruby set adds a second layer above the client, for programs whose prompts are declared rather than hand-written:

```
signature   typed inputs, typed outputs, instructions — no provider, no prompting
  ↓
predict     adapters render signatures into messages and parse replies back
  ↓
evaluate    scores a module over a labeled devset
```

`signature` depends on nothing else in the repo. `predict` builds on it and on `client`. `evaluate` builds on `predict`. An optimizer layer, which tunes instructions and few-shot demonstrations against an evaluation score, is the intended next step.

## Applications

See `apps/` for demo applications built on these libraries.

## License

MIT. Enterprise support available from [Durable Programming](https://durableprogramming.com).
