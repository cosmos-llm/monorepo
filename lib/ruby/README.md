# Cosmos LLM — Ruby Libraries

Five gems with a layered architecture. Each can be used independently; higher-level gems compose on lower-level ones.

## Gems

### `cosmos-llm` — [cosmos-llm-ruby-client](cosmos-llm-ruby-client/)

Unified client for 15+ LLM providers: OpenAI, Anthropic, Google, Cohere, Mistral, Groq, and more.

- Chat completions and streaming
- Embeddings
- Provider fallback chains
- CLI tool (`exe/cosmos-llm`)

```ruby
require 'cosmos/llm'

client = Cosmos::Llm::Client.new(provider: :anthropic)
response = client.complete(messages: [{ role: 'user', content: 'Hello' }])
```

### `cosmos-llm-context` — [cosmos-llm-ruby-context](cosmos-llm-ruby-context/)

DSL for modeling agentic LLM contexts. Renders to Anthropic, OpenAI, JSON, or XML message formats.

- Immutable context blocks
- Virtual filesystem integration
- Multi-format output renderers

### `cosmos-llm-tool` — [cosmos-llm-ruby-tool](cosmos-llm-ruby-tool/)

Tool registration and execution for LLM function calling. Wraps arbitrary Ruby code as tools an LLM can invoke.

### `cosmos-llm-tool-preset` — [cosmos-llm-ruby-tool-preset](cosmos-llm-ruby-tool-preset/)

Pre-built tools for common operations:

- File read and write (sandboxed via virtual filesystem)
- Grep search
- Web fetch

### `cosmos-llm-virtual-filesystem` — [cosmos-llm-ruby-virtual-filesystem](cosmos-llm-ruby-virtual-filesystem/)

Hierarchical in-memory filesystem. The foundational layer — no upstream dependencies within this repo.

## Dependency graph

```
cosmos-llm-virtual-filesystem
        ↑
cosmos-llm-context
        ↑              ↑
cosmos-llm-tool    cosmos-llm (client)
        ↑
cosmos-llm-tool-preset
```

## Installing from GitHub

These gems are not yet published to RubyGems. Install them directly from the [cosmos-llm/monorepo](https://github.com/cosmos-llm/monorepo) GitHub repository using Bundler's `github:` option.

Declare each gem you need with its path inside the monorepo:

```ruby
source "https://rubygems.org"

gem "cosmos-llm",                    github: "cosmos-llm/monorepo", glob: "lib/ruby/cosmos-llm-ruby-client/*.gemspec"
gem "cosmos-llm-context",            github: "cosmos-llm/monorepo", glob: "lib/ruby/cosmos-llm-ruby-context/*.gemspec"
gem "cosmos-llm-tool",               github: "cosmos-llm/monorepo", glob: "lib/ruby/cosmos-llm-ruby-tool/*.gemspec"
gem "cosmos-llm-tool-preset",        github: "cosmos-llm/monorepo", glob: "lib/ruby/cosmos-llm-ruby-tool-preset/*.gemspec"
gem "cosmos-llm-virtual-filesystem", github: "cosmos-llm/monorepo", glob: "lib/ruby/cosmos-llm-ruby-virtual-filesystem/*.gemspec"
```

You only need the lines for gems you actually use. A project that only needs the client:

```ruby
gem "cosmos-llm", github: "cosmos-llm/monorepo", glob: "lib/ruby/cosmos-llm-ruby-client/*.gemspec"
```

To pin to a specific commit or branch, add `ref:` or `branch:`:

```ruby
gem "cosmos-llm", github: "cosmos-llm/monorepo",
                  glob:   "lib/ruby/cosmos-llm-ruby-client/*.gemspec",
                  branch: "main"
```

Then run `bundle install` as usual.

## Development

Each gem has its own `devenv.nix` for a reproducible development shell. Tests use Minitest.

```sh
cd cosmos-llm-ruby-client
bundle install
bundle exec rake test
```

## License

MIT. Enterprise support from [Durable Programming](https://durableprogramming.com).
