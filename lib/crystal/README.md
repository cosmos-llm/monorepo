# Cosmos LLM — Crystal Libraries

## Shards

### `cosmos-llm` — [cosmos-llm-crystal](cosmos-llm-crystal/)

Unified client for OpenAI and Anthropic.

- Chat completions via the stdlib `HTTP::Client` — no third-party dependencies
- Provider-normalized tool calling
- Model discovery
- Configuration from `CLLM__<PROVIDER>__<SETTING>` environment variables

```crystal
require "cosmos-llm"

client = CosmosLLM::Client.new("anthropic").with_model("claude-sonnet-4-6")
puts client.complete("Hello!")
```

The API mirrors the Rust crate and Ruby gem of the same name: `Client`,
`Config`, `CompletionRequest`/`CompletionResponse`, and a `Providers::Base`
class to extend.

## Roadmap

The Crystal set currently covers the client layer only. The `context`, `tool`,
`tool-preset`, and `virtual-filesystem` layers that exist in Ruby and Rust have
not been ported yet.

## Development

Requires Crystal 1.10+.

```sh
cd cosmos-llm-crystal
crystal spec
```

## License

MIT. Enterprise support from [Durable Programming](https://durableprogramming.com).
