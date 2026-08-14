# cosmos-llm-predict

Runs [cosmos-llm-signature](../cosmos-llm-ruby-signature) signatures against
language models.

An adapter renders the signature and its inputs into messages, the client sends
them, and the adapter parses the reply back into typed values. The result is a
`Prediction`.

## Installation

```ruby
gem 'cosmos-llm-predict'
```

## Usage

```ruby
require 'cosmos/llm'
require 'cosmos/llm/predict'

Cosmos::Llm::Predict.configure do |settings|
  settings.client = Cosmos::Llm::Client.new(:anthropic, model: 'claude-opus-4')
end

answer = Cosmos::Llm::Predict::Predict.new('question -> answer')
prediction = answer.call(question: 'What is 2+2?')

prediction.answer      # => '4'
prediction.completion  # => the raw model output
prediction.usage       # => { 'input_tokens' => 34, ... }
```

Anything responding to `completion(**params)` works as a client, so tests can
pass a double instead of hitting the network.

## Adapters

Two prompting strategies, same signature.

`ChatAdapter` (default) delimits fields with section headers:

```
[[ ## summary ## ]]
The document argues that ...

[[ ## confidence ## ]]
0.82

[[ ## done ## ]]
```

It asks less of the model than strict JSON — no escaping, no balanced braces,
multi-line prose needs no special handling — which makes it the safer default.

`JsonAdapter` asks for one JSON object. Where a provider supports native
structured output, it sends the signature's schema so the format is enforced
rather than requested:

```ruby
adapter = Cosmos::Llm::Predict::Adapters::JsonAdapter.new(native_structured_output: true)
Cosmos::Llm::Predict::Predict.new(Summarize, adapter: adapter)
```

Extraction tolerates fenced code blocks and surrounding prose, since models add
both regardless of instructions.

## Parse retries

When a reply cannot be parsed, the call is retried with the malformed output and
the parse error fed back as a correction turn. Models usually fix a format
mistake when shown it, and this costs one extra call rather than failing the run.
Set `settings.max_parse_retries` to control it (default 1).

## Few-shot demonstrations

```ruby
demos = [Example.new(document: '...', summary: '...', confidence: 0.9)]
predictor = Cosmos::Llm::Predict::Predict.new(Summarize, demos: demos)
```

Demos are rendered as alternating user/assistant turns in the adapter's own
format, so they teach the output contract as well as the task.

## Caching

Evaluation loops and optimizers replay identical calls constantly. The cache
checks an in-process LRU, then a directory of JSON files keyed by a digest of the
request:

```ruby
cache = Cosmos::Llm::Predict::Cache.new(directory: '~/.cache/cosmos-llm')
Cosmos::Llm::Predict.configure { |settings| settings.cache = cache }

prediction.cached?  # => true on a repeat call
```

Entries are JSON, not marshalled objects — a cache directory is a file a later
process trusts, and JSON keeps arbitrary Ruby objects out of it.

The key covers the signature's instructions and fields, the adapter, and the
request, so changing any of them misses correctly.

## Modules

`Predict` is one call. `Module` is the composable unit built from it:

```ruby
class Rag < Cosmos::Llm::Predict::Module
  def initialize
    super
    @generate = Cosmos::Llm::Predict::Predict.new('context, question -> answer')
  end

  def forward(question:)
    @generate.call(context: retrieve(question), question: question)
  end
end

Rag.new.named_predictors  # => { "generate" => #<Predict ...> }
```

`named_predictors` walks instance variables, arrays, and hashes to find every
nested predictor. That is what lets an optimizer install demonstrations into a
program without knowing its shape.

### ChainOfThought

Derives a signature with a `reasoning` field ahead of the declared outputs.
Because adapters render fields in order, the model reasons first and answers
after — the whole mechanism is field ordering.

```ruby
cot = Cosmos::Llm::Predict::ChainOfThought.new('question -> answer')
prediction = cot.call(question: 'If a train leaves at 3pm ...')
prediction.reasoning  # => 'The train travels for two hours, so ...'
```

### BestOfN

Samples a module at raised temperature and keeps the best-scoring result. A
threshold short-circuits the rest, so an easy input costs one call:

```ruby
sampler = Cosmos::Llm::Predict::BestOfN.new(predictor, n: 5, threshold: 1.0) do |_inputs, pred|
  valid_json?(pred.answer) ? 1.0 : 0.0
end
```

Attempts that raise are skipped rather than aborting the run.

## Development

```
rake test      # minitest
rake rubocop
rake doc       # yard
```

## License

MIT. Copyright (c) 2025 Durable Programming, LLC.
