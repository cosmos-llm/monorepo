# cosmos-llm

A unified Crystal client for multiple LLM providers. Part of the [Cosmos-LLM](https://github.com/cosmos-llm) monorepo.

## Supported providers

| Name | Completion | Tool calling | Models |
|---|---|---|---|
| `openai` | ✓ | ✓ | ✓ |
| `anthropic` | ✓ | ✓ | ✓ (static list) |

`Client#can_stream?` reports whether a provider's API supports streaming
(`true` for OpenAI, `false` for Anthropic), matching the Rust crate. Streaming
response handling is not implemented in either yet — the flag is capability
metadata, not a working transport.

## Installation

Add the dependency to your `shard.yml`:

```yaml
dependencies:
  cosmos-llm:
    github: durableprogramming/cosmos-llm-crystal
```

Then run `shards install`.

## Usage

```crystal
require "cosmos-llm"

# API key from env: OPENAI_API_KEY or CLLM__OPENAI__API_KEY
client = CosmosLLM::Client.new("openai").with_model("gpt-4o")

# One-shot completion
puts client.complete("What is the capital of France?")

# Full chat with a system message
response = client.chat(
  [
    CosmosLLM::Message.system("You are a concise assistant."),
    CosmosLLM::Message.user("Name three planets."),
  ],
  temperature: 0.5,
  max_tokens: 256,
)

puts response.text
```

For full control, build a `CompletionRequest` yourself:

```crystal
request = CosmosLLM::CompletionRequest.new("gpt-4o", [CosmosLLM::Message.user("Hi")])
  .with_temperature(0.5)
  .with_max_tokens(256)
  .with_stop(["END"])

response = client.completion(request)
```

### Tool calling

Tool schemas are passed through in provider-native format. Responses are
normalized: `ToolCall#input` is always a parsed `JSON::Any`, whether the
provider sent a JSON string (OpenAI) or an object (Anthropic).

```crystal
request = CosmosLLM::CompletionRequest.new("gpt-4o", messages)
  .with_tools([{
    "type"     => "function",
    "function" => {
      "name"        => "get_weather",
      "description" => "Look up the weather for a city",
      "parameters"  => {
        "type"       => "object",
        "properties" => {"city" => {"type" => "string"}},
        "required"   => ["city"],
      },
    },
  }])
  .with_tool_choice("auto")

response = client.completion(request)

if response.tool_use?
  response.tool_calls.each do |call|
    puts "#{call.name} -> #{call.input}"
  end
end
```

## Configuration

### Programmatic

```crystal
config = CosmosLLM::Config.new
config.set_api_key("anthropic", "sk-ant-...")
config.set_model("anthropic", "claude-sonnet-4-6")
config.default_provider = "anthropic"

client = CosmosLLM::Client.from_config(config)
```

### Environment variables

```bash
export CLLM__OPENAI__API_KEY=sk-...
export CLLM__ANTHROPIC__API_KEY=sk-ant-...
export CLLM__OPENAI__MODEL=gpt-4o
```

Provider-native variables (`OPENAI_API_KEY`, `ANTHROPIC_API_KEY`) are read too,
and take precedence over their `CLLM__` equivalents.

Unrecognized `CLLM__<PROVIDER>__<SETTING>` keys land in
`Config#provider(name).extra` rather than being discarded.

## Errors

Every failure raises a subclass of `CosmosLLM::Error`:

| Class | Raised when |
|---|---|
| `AuthenticationError` | key missing or rejected (HTTP 401) |
| `RateLimitError` | HTTP 429 |
| `InvalidRequestError` | HTTP 400 or 404 |
| `ServerError` | HTTP 5xx |
| `UnsupportedProviderError` | unknown provider name |
| `ConfigurationError` | no model set |
| `InvalidResponseError` | unparseable or unexpected response body |

## Adding a provider

Subclass `CosmosLLM::Providers::Base`, implement `#completion` and `#models`,
then register the name in `CosmosLLM::Providers.resolve`. The base class
supplies `#request_json` and `#handle_error`, so a new backend only has to deal
with its own request and response shapes.

## Development Setup

This project uses [devenv](https://devenv.sh/) for reproducible environments.

### Prerequisites

- [Nix](https://nixos.org/download.html)
- [devenv](https://devenv.sh/getting-started/)

### Getting started

```bash
git clone <repo>
cd cosmos-llm-crystal
devenv shell

crystal spec
crystal tool format --check .
crystal docs
```

The specs are hermetic — they exercise response mapping, configuration, and
error handling without making network calls.

## License

MIT
