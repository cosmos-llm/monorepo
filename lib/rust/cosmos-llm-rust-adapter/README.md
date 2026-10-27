# cosmos-llm-adapter

Prompting strategies for [`cosmos-llm-signature`](../cosmos-llm-rust-signature/)
signatures: render a signature and its inputs into chat messages, and parse the
model's reply back into typed outputs.

An adapter is the *how* to a signature's *what*. The signature says "given a
`question`, produce an integer `answer`"; the adapter decides whether the model
is asked for `[[ ## answer ## ]]` sections or a JSON object, and how to read
that back. Swapping the adapter changes the prompt without touching the
signature or the code that calls it.

This crate makes no network calls and has no async runtime. It produces
messages and request parameters, and consumes reply text, so every strategy is
testable offline and works with any client.

```rust
use cosmos_llm_adapter::{adapter, Adapter};
use cosmos_llm_signature::Signature;
use serde_json::{json, Value};

let sig = Signature::parse("question -> answer: int")?;
let chat = adapter("chat")?;

let inputs = json!({ "question": "What is 2+2?" });
let messages = chat.format(&sig, inputs.as_object().unwrap(), &[])?;
// ...send `messages` with any client, then:
let out = chat.parse(&sig, "[[ ## answer ## ]]\n4\n[[ ## done ## ]]")?;
assert_eq!(out["answer"], json!(4));
```

## Built-in adapters

| Name          | Asks for                                                  |
|---------------|-----------------------------------------------------------|
| `chat`        | each field under its own `[[ ## name ## ]]` header        |
| `json`        | a single JSON object, requested in the prompt             |
| `json_schema` | the same, plus the schema sent as `response_format`       |

**`chat` is the default** because it puts nothing between the model and free
text. A multi-paragraph answer with code blocks and quotation marks needs no
escaping, and the headers are unlikely to appear in it by accident.

**`json`** is better when outputs are structured — arrays, objects, several
small typed fields. Parsing tries the whole reply, then a fenced code block,
then the span from the first `{` to the last `}`, because models wrap JSON in
prose despite being told not to.

**`json_schema`** adds the output schema as an OpenAI-style `response_format`,
returned from `Adapter::request_params` for the caller to merge into the
request body. Providers that support structured output then enforce the shape
themselves; the prompt instructions stay in place for the ones that don't.

Parse failures come back as `AdapterError::Parse`, carrying both a message and
the reply verbatim. The usual recovery is to show the model what it wrote and
the message, and ask again; the message is worded for that.

## Demonstrations

`format` takes demos as plain value maps holding both input and output fields.
Each becomes a user turn and an assistant turn, rendered in exactly the layout
`parse` reads, so the model sees worked examples of the format. Keys the
signature does not declare are ignored.

## Several objects in one reply

`json_stream::parse_all` handles replies with one JSON object per record —
newline-separated, comma-separated, or fenced as `jsonl`. It tracks brace depth
outside strings, so it does not care what separates the objects, and it reports
objects that fail to decode without losing the rest.

## Swapping an adapter

Adapters are looked up by name through `inventory` registrations. Register your
own under a new name to add a strategy, or under a built-in's name to replace it
everywhere the name is used, including the CLI. A registration without an
explicit priority outranks the built-ins, so nothing else is needed:

```rust
use cosmos_llm_adapter::{Adapter, AdapterPlugin};
use cosmos_llm_adapter::registry::meta;

fn build() -> Box<dyn Adapter> {
    Box::new(MyChatAdapter::default())
}

cosmos_llm_adapter::registry::inventory::submit! {
    AdapterPlugin { meta: meta!("chat").summary("Our house prompt"), build }
}
```

Only `format_instructions`, `render_inputs`, `render_demo_outputs`, and
`extract` are required; `format`, `parse`, `system_prompt`, and
`field_description` have defaults built from them, and any can be overridden.

`inventory` collects registrations from every crate linked into the binary. A
plugin crate that is a dependency but never referenced may be dropped by the
linker, so name it once:

```rust
use my_adapter_plugins as _;
```

## Command line

Built with the `cli` feature:

```sh
cargo install --path . --features cli
```

```sh
# The messages a model would receive, as a JSON array of {role, content}.
cosmos-llm-adapter format 'question -> answer' '{"question": "What is 2+2?"}'

# Inputs from stdin, demonstrations from a JSONL file.
echo '{"q": "3+3"}' | cosmos-llm-adapter format 'q -> a: int' --demos demos.jsonl

# With --params, prints {"messages": [...], "params": {...}}.
cosmos-llm-adapter format 'q -> a: int' --adapter json_schema --params '{"q": "hi"}'

# Parse a saved reply, from a file or stdin.
cosmos-llm-adapter parse 'question -> answer: int' reply.txt
some-model-cli < prompt.json | cosmos-llm-adapter parse 'q -> a' -

# What is registered, and what shadows what.
cosmos-llm-adapter plugins
```

A signature spec is a shorthand string, `@file.json`, or the name of a
registered signature. Exit status is 0 on success, 1 when the command fails —
including a reply that does not parse — and 2 on a usage error.

To get the CLI with your own adapters included, make a binary that links them
and calls `cosmos_llm_adapter::cli::main()`.

## Requirements

Rust 1.75 or later.

## License

MIT. Enterprise support from [Durable Programming](https://durableprogramming.com).
