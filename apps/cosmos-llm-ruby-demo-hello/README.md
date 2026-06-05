# cosmos-llm-ruby-demo-hello

A minimal interactive CLI chatbot built on [cosmos-llm](https://github.com/cosmos-llm/cosmos-llm).

## Usage

```bash
bundle exec cosmos-llm-ruby-demo-hello
bundle exec cosmos-llm-ruby-demo-hello -p openai -m gpt-4o
bundle exec cosmos-llm-ruby-demo-hello -s "You are a pirate. Respond accordingly."
```

Options:

| Flag | Default | Description |
|------|---------|-------------|
| `-p PROVIDER` | `anthropic` | LLM provider name |
| `-m MODEL` | `claude-haiku-4-5-20251001` | Model identifier |
| `-s PROMPT` | _(none)_ | System prompt |

In-session commands:

- `/reset` — clear conversation history
- `exit` / `quit` / `q` — quit

## API keys

Set the appropriate env var for your provider, e.g.:

```bash
export DLLM__ANTHROPIC__API_KEY=sk-ant-...
export DLLM__OPENAI__API_KEY=sk-...
```

## Development

```bash
bundle install
bundle exec rake test
bundle exec rubocop
```

## License

MIT
