# cosmos-llm-evaluate

Scores [cosmos-llm-predict](../cosmos-llm-ruby-predict) modules against labeled
devsets.

An evaluation answers one question: over these examples, how often does this
program get it right? That number is what makes a change to a prompt or a model
an experiment rather than a guess.

## Installation

```ruby
gem 'cosmos-llm-evaluate'
```

## Usage

```ruby
require 'cosmos/llm/evaluate'

Example = Cosmos::Llm::Predict::Example
Metrics = Cosmos::Llm::Evaluate::Metrics

devset = [
  Example.new(country: 'France', capital: 'Paris').with_inputs(:country),
  Example.new(country: 'Japan',  capital: 'Tokyo').with_inputs(:country)
]

result = Cosmos::Llm::Evaluate.run(
  predictor, devset,
  metric: Metrics.exact_match(:capital),
  threads: 8
)

result.score    # => 0.5
result.summary  # => "50.0% (1/2 passed)"
```

`with_inputs` marks which fields the module receives; everything else is the
label the metric compares against.

## The result

The aggregate rarely tells you what to fix, so every per-example row is kept:

```ruby
result.failures.each do |row|
  puts "#{row.example[:country]}: got #{row.prediction[:capital]}"
end

result.errors     # rows where the module raised
result.passes(threshold: 0.8)
result.to_h       # => { score:, size:, passed:, failed:, errored: }
```

## Metrics

A metric takes `(example, prediction)` and returns a number. Anything responding
to `call` works; these are a convenience.

| Metric | Scores |
|---|---|
| `exact_match(field)` | 1.0 on an exact match after normalization |
| `f1(field)` | token-level F1 overlap |
| `contains(field)` | 1.0 when the label appears in the prediction |
| `set_overlap(field)` | fraction of expected list items found |
| `all_of(*metrics)` | 1.0 only when every metric passes |
| `average(*metrics, weights:)` | weighted mean |

Normalization lowercases, strips punctuation, collapses whitespace, and drops
leading articles — the differences that almost never matter:

```ruby
Metrics.exact_match(:answer).call(
  Example.new(answer: 'the Eiffel Tower'),
  Prediction.new(answer: 'Eiffel Tower.')
)  # => 1.0
```

Pass `normalize: false` when they do matter.

## Failure handling

Examples are independent, so they run concurrently up to `threads`. The work is
almost entirely waiting on HTTP, which is where Ruby threads help despite the
GVL.

A module that raises on one example does not abort the run: the failure becomes a
zero-scoring row and evaluation continues. Losing a whole devset sweep to one bad
row is worse than scoring it zero. A raising *metric* is treated the same way.

Set `max_errors` to draw a line:

```ruby
Cosmos::Llm::Evaluate.run(program, devset, metric: metric, max_errors: 5)
# raises TooManyErrorsError, carrying the recorded failures
```

Error rows count as zero in the aggregate rather than being dropped — a program
that crashes on a third of the devset has a real problem, and averaging over the
survivors would hide it.

## Progress

```ruby
Cosmos::Llm::Evaluate.run(
  program, devset,
  metric: metric,
  progress: ->(done, total) { print "\r#{done}/#{total}" }
)
```

## Development

```
rake test      # minitest
rake rubocop
rake doc       # yard
```

## License

MIT. Copyright (c) 2025 Durable Programming, LLC.
