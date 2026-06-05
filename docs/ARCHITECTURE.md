# Architecture

## Overview

Cosmos-LLM is a monorepo containing a layered set of libraries and applications for interacting with LLMs. The core idea is a unified provider abstraction: one API surface, many backends. Ruby is the primary language; there is also a JavaScript/TypeScript client with the same design.

The repo has five top-level sections:

| Directory | Contents |
|-----------|----------|
| `lib/` | Reusable libraries (Ruby gems, JS package) |
| `apps/` | Standalone CLI applications built on the libraries |
| `external/` | Vendored third-party projects for reference |
| `private/` | Private development work and model registry |
| `ref/` | Coding standards and writing guidelines |

---

## Libraries (`lib/`)

### `lib/ruby/cosmos-llm-ruby-client`

The central Ruby library. Gem name: `cosmos-llm`. Requires Ruby >= 2.6.0.

Entry point is `Cosmos::Llm` (`lib/cosmos/llm.rb`), which exposes module-level convenience methods (`complete`, `chat`, `models`) and delegates to `Cosmos::Llm::Client`.

**Provider support** (16 providers):

`anthropic`, `openai`, `azure_openai`, `google`, `cohere`, `mistral`, `groq`, `fireworks`, `together`, `deepseek`, `openrouter`, `perplexity`, `xai`, `huggingface`, `opencode`

Each provider lives in `lib/cosmos/llm/providers/<name>.rb` and inherits from `providers/base.rb`. The base class defines the interface: `complete`, `chat`, `stream`, `embeddings`, `models`.

**Configuration** uses the `DLLM__` environment variable prefix (e.g. `DLLM__OPENAI__API_KEY`) or a block-based programmatic API:

```ruby
Cosmos::Llm.configure do |config|
  config.openai.api_key = 'sk-...'
  config.default_provider = 'anthropic'
end
```

**Key source files:**

```
lib/cosmos/llm.rb                  # Module entry point, convenience methods
lib/cosmos/llm/client.rb           # Client class
lib/cosmos/llm/configuration.rb    # Config management
lib/cosmos/llm/providers/base.rb   # Provider interface
lib/cosmos/llm/providers/*.rb      # 16 provider implementations
lib/cosmos/llm/http_client.rb      # HTTP transport (Faraday)
lib/cosmos/llm/cli.rb              # cllm CLI tool
lib/cosmos/llm/response_helpers.rb # Token usage, cost estimation
lib/cosmos/llm/provider_utilities.rb # Provider discovery, fallbacks
lib/cosmos/llm/convenience.rb      # Global functions (skipped via DLLM_NO_CONVENIENCE)
lib/cosmos/llm/errors.rb           # Error hierarchy
```

**Runtime dependencies:** `faraday`, `zeitwerk`, `event_stream_parser`, `thor`, `highline`, `ostruct`, `json`

---

### `lib/ruby/cosmos-llm-ruby-context`

Gem: `cosmos-llm-context`. Builds structured prompts for agentic LLM interactions.

The main class is `Cosmos::Llm::Context::Builder`, which takes a DSL block and assembles an ordered list of `Block` objects. A `Block` holds a name (e.g. `:system`, `:user`), content, and metadata. Builders can also attach a virtual filesystem to give the context a file tree.

Rendering is pluggable. Five built-in renderers: `default`, `xml`, `json`, `anthropic`, `openai`. Custom renderers can be registered at runtime. The builder uses `BuilderMixins` for extensibility without deep inheritance.

All core objects (`Block`, `Filesystem`, `File`) are frozen after construction — safe for concurrent use.

```
lib/cosmos/llm/context/builder.rb          # DSL entry point
lib/cosmos/llm/context/block.rb            # Content block
lib/cosmos/llm/context/filesystem.rb       # Virtual filesystem node
lib/cosmos/llm/context/renderers.rb        # Renderer registry
lib/cosmos/llm/context/renderers/*.rb      # 5 renderer implementations
lib/cosmos/llm/context/builder_mixins/*.rb # Mixin extensions
lib/cosmos/llm/context/errors.rb
```

**Runtime dependency:** `cosmos-llm-virtual-filesystem`, `zeitwerk`

---

### `lib/ruby/cosmos-llm-ruby-virtual-filesystem`

Gem: `cosmos-llm-virtual-filesystem`. A minimal in-memory filesystem used by the context library and the tool presets.

Provides nested directory/file structures with path-based navigation, metadata, and content management. The core class is `Cosmos::Llm::VirtualFilesystem::Filesystem`. Objects are immutable once built.

```
lib/cosmos/llm/virtual_filesystem/filesystem.rb
lib/cosmos/llm/virtual_filesystem/errors.rb
```

**Runtime dependency:** `zeitwerk`

---

### `lib/ruby/cosmos-llm-ruby-tool`

Gem: `cosmos-llm-tool`. The function-calling layer.

Defines `Tool::Definition` (schema + handler), `Tool::Registry` (named lookup), `Tool::Executor` (invokes a tool from an LLM response), and `Tool::Parameter` (validates arguments). This is the base layer; actual tool implementations live in `cosmos-llm-tool-preset`.

```
lib/cosmos/llm/tool/definition.rb
lib/cosmos/llm/tool/registry.rb
lib/cosmos/llm/tool/executor.rb
lib/cosmos/llm/tool/parameter.rb
lib/cosmos/llm/tool/schemas.rb
lib/cosmos/llm/tool/errors.rb
```

**Runtime dependency:** `cosmos-llm`, `zeitwerk`

---

### `lib/ruby/cosmos-llm-ruby-tool-preset`

Gem: `cosmos-llm-tool-preset`. Ready-to-use tools that plug into the tool framework.

All tools operate through the virtual filesystem abstraction, keeping file access sandboxed and auditable.

| Tool | Purpose |
|------|---------|
| `read` | Read a file |
| `write` | Write a file |
| `list` | List a directory |
| `grep` | Text search |
| `glob` | Pattern matching |
| `jq` | JSON processing |
| `webfetch` | HTTP GET |

**Runtime dependency:** `cosmos-llm-tool`, `cosmos-llm-virtual-filesystem`, `zeitwerk`

---

### `lib/js/cosmos-llm-js-client`

TypeScript package: `cosmos-llm` v0.1.0. Mirrors the Ruby client's design for JavaScript/TypeScript consumers.

Entry point exports `CosmosLlm` (extends `Client`), `configure()`, `getConfiguration()`, and all provider/error types. Currently supports OpenAI and Anthropic; the provider interface is the same pluggable pattern as Ruby.

```
src/index.ts          # Exports, CosmosLlm class, configure()
src/client.ts         # Client class
src/configuration.ts  # Configuration
src/http-client.ts    # HTTP transport (node-fetch)
src/errors.ts         # AuthenticationError, RateLimitError, InvalidRequestError, etc.
src/providers/base.ts # Provider interface
src/providers/openai.ts
src/providers/anthropic.ts
```

**Runtime dependency:** `node-fetch`

**Examples** are in `examples/`: `simple-completion.ts`, `streaming.ts`, `fluent-api.ts`, `anthropic.ts`.

---

## Applications (`apps/`)

### `apps/cosmos-llm-hello`

A minimal interactive chat CLI. Demonstrates the Ruby client at its simplest: pick a provider, pick a model, chat. Supports `/reset` to clear history and `exit`/`quit`/`q` to stop.

```
exe/cosmos-llm-hello
lib/cosmos_llm_hello/cli.rb          # Thor-based argument handling
lib/cosmos_llm_hello/chat_session.rb # Conversation loop
lib/cosmos_llm_hello/error.rb
```

Usage: `cosmos-llm-hello -p openai -m gpt-4o -s "You are a pirate"`

**Dependency:** `cosmos-llm >= 0.1.4`

---

### `apps/cosmos-llm-markdowner`

An agentic CLI that uses an LLM agent to generate markdown files. The key design point is explicit permission scoping: callers specify which paths the agent may read (`--in`) and write (`--out`, `--out-dir`). Filesystem access runs through `cosmos-llm-tool-preset`, so the agent cannot escape the declared boundaries.

**Dependencies:** `cosmos-llm >= 0.1.4`, `cosmos-llm-tool >= 0.1.0`

---

## Dependency Graph

```
cosmos-llm-virtual-filesystem   (no internal deps)
       │
       ├── cosmos-llm-ruby-context
       └── cosmos-llm-ruby-tool-preset
                   │
cosmos-llm-ruby-client ──────── cosmos-llm-ruby-tool
       │                                │
       │                    cosmos-llm-ruby-tool-preset
       │
       ├── apps/cosmos-llm-hello
       └── apps/cosmos-llm-markdowner ── cosmos-llm-ruby-tool

cosmos-llm-js-client            (standalone, no internal deps)
```

---

## Reference Materials (`ref/`)

Two submodules checked out as directories:

- `ref/djberube-writing-guide` — Writing style rules for documentation and articles.
- `ref/durableprogramming-coding-standards` — Coding standards covering Ruby gems, npm packages, Rust crates, Rails+Inertia.js apps, CLI design, and documentation conventions.

These inform how new libraries in this repo are built but are not runtime dependencies.

---

## External Projects (`external/`)

Vendored for reference and potential integration study:

- `external/opencode` — Open-source AI coding agent (TypeScript, terminal UI + desktop app, MCP support).
- `external/gemini-cli` — Google's Gemini CLI agent (TypeScript, Google Search integration, MCP support).

Neither is imported by the libraries or apps in this repo.

---

## Private Work (`private/`)

- `private/cosmos-llm` — Development staging copy of the main Ruby client gem.
- `private/cosmos-llm-registry` — A structured catalog of LLM models from 13+ providers. Each provider has a `catalog.md` (specs, pricing, usage notes), an `openapi.yaml`, and a `models.jsonl` file. Covered providers: Anthropic, Azure OpenAI, Cohere, DeepSeek, Google, Groq, Mistral, OpenAI, OpenCode Zen, OpenRouter, Perplexity, Together, xAI.
- `private/durable-llm` — Earlier iteration of the client gem under the `durable-llm` name; superseded by `cosmos-llm`.

---

## Key Design Patterns

**Provider abstraction.** Every provider implements the same interface (`complete`, `chat`, `stream`, `embeddings`, `models`). The client resolves a provider by name and delegates. Adding a new provider means adding one file under `providers/` and registering it — nothing else changes.

**Builder DSL.** `cosmos-llm-ruby-context` uses `instance_eval` to give callers a clean block syntax. Mixins extend the builder without inheritance.

**Immutable core objects.** Blocks, virtual filesystem nodes, and files are frozen after construction. This makes contexts safe to build once and render in multiple threads.

**Sandboxed tool execution.** The tool preset library routes all I/O through the virtual filesystem. An agent can only touch paths it was explicitly given, which makes the tools suitable for untrusted or partially-trusted agentic loops.

**Environment-first configuration.** The `DLLM__PROVIDER__KEY` convention means secrets never need to appear in code. Programmatic configuration is also supported for cases where env vars are inconvenient.
