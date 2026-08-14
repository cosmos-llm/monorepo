# cosmos-llm-signature

Declarative typed input/output signatures for LLM calls.

A signature describes **what** one LLM call does — named typed inputs, named
typed outputs, and instructions — without saying how to prompt for it. Rendering
a signature into messages and parsing the reply back into typed values is the
job of an adapter in
[cosmos-llm-predict](../cosmos-llm-ruby-predict), which means the same signature
works across every provider and prompting strategy.

## Installation

```ruby
gem 'cosmos-llm-signature'
```

## Declaring a signature

The subclass form is the most explicit:

```ruby
class Summarize < Cosmos::Llm::Signature
  instructions 'Summarize the document for a technical reader.'

  input  :document, String, desc: 'raw text to summarize'
  output :summary, String, desc: 'three sentences, no preamble'
  output :confidence, Float, desc: 'certainty between 0 and 1'
end

Summarize.input_fields.keys   # => [:document]
Summarize.output_fields.keys  # => [:summary, :confidence]
```

For signatures built at runtime, use the block form:

```ruby
sig = Cosmos::Llm::Signature.build do
  input  :question
  output :answer
end
```

And for quick work, the shorthand string:

```ruby
Cosmos::Llm::Signature.parse('question -> answer')
Cosmos::Llm::Signature.parse('document, style -> summary: string, score: number')
Cosmos::Llm::Signature.parse('text -> tags: array[string]')
```

`Signature.coerce` accepts either a class or a shorthand string, which is what
API boundaries should use.

## Types

Fields take Ruby classes or symbol shorthands:

| Declared | Canonical | JSON schema |
|---|---|---|
| `String`, `:string`, `:str` | `:string` | `string` |
| `Integer`, `:integer`, `:int` | `:integer` | `integer` |
| `Float`, `:number`, `:float` | `:number` | `number` |
| `:boolean`, `:bool` | `:boolean` | `boolean` |
| `Array`, `:array`, `:list` | `:array` | `array` |
| `Hash`, `:object` | `:object` | `object` |

Arrays may declare an element type, and any field may constrain its values:

```ruby
output :tags, Array, of: String
output :mood, String, enum: %w[positive neutral negative]
output :note, String, required: false
output :retries, Integer, default: 0
```

## Coercion

Models return text. `coerce_outputs` turns that text into the declared types,
parsing numbers, booleans, and JSON as needed:

```ruby
Summarize.coerce_outputs('summary' => 'It argues X.', 'confidence' => '0.82')
# => { summary: 'It argues X.', confidence: 0.82 }
```

A missing required field raises `MissingFieldError`; a value that cannot be
converted raises `CoercionError`. Optional fields are simply omitted, and fields
with a default fall back to it.

## JSON schema

`output_json_schema` derives the schema handed to providers that support
structured output:

```ruby
Summarize.output_json_schema
# => { 'type' => 'object',
#      'properties' => { 'summary' => { 'type' => 'string', ... }, ... },
#      'required' => ['summary', 'confidence'],
#      'additionalProperties' => false }
```

## Variants

Signatures are immutable in practice: `with_instructions` and `derive` return
new classes rather than mutating the receiver. Optimizers depend on this, since
they try variants against a program they were handed and must not disturb it.

```ruby
Terse = Summarize.with_instructions('Be terse.')
WithReasoning = Summarize.derive { output :reasoning, String }

Summarize.instructions  # unchanged
```

`cache_signature` returns a stable description of the fields and instructions,
suitable for building a cache key that survives process restarts.

## Development

```
rake test      # minitest
rake rubocop
rake doc       # yard
```

## License

MIT. Copyright (c) 2025 Durable Programming, LLC.
