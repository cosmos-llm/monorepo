# cosmos-llm-signature

Declarative descriptions of LLM calls: named typed inputs, named typed outputs,
and the instructions that connect them. A Rust port of the Ruby
`cosmos-llm-signature` gem.

A signature says *what* a call does. It does not say how to prompt for it. That
is an adapter's job ([`cosmos-llm-adapter`](../cosmos-llm-rust-adapter/)), and
making the call is a predictor's
([`cosmos-llm-predict`](../cosmos-llm-rust-predict/)). Keeping the description
separate is what lets one signature work across providers and prompting
strategies, and what lets an optimizer try instruction variants against fixed
fields.

```rust
use cosmos_llm_signature::Signature;
use serde_json::json;

let sig = Signature::parse("document -> summary, tags: array[string]")?;

// The JSON schema a provider with structured output receives.
let schema = sig.output_json_schema()?;

// What a model's raw reply becomes.
let out = sig.coerce_outputs(&json!({
    "summary": "Short.",
    "tags": "[\"rust\", \"llm\"]",
}))?;
assert_eq!(out["tags"], json!(["rust", "llm"]));
```

## Three ways to declare one

**Shorthand**, for most calls:

```text
question -> answer
document, style -> summary: string, confidence: number
text -> tags: array[string]
```

**Builder**, for descriptions, enums, optional fields, and defaults:

```rust
use cosmos_llm_signature::{Field, Signature};

let sig = Signature::builder()
    .instructions("Route the support ticket to a team.")
    .field(Field::input("ticket").desc("the customer's message, verbatim"))
    .field(Field::output("team").one_of(["billing", "platform", "mobile"]))
    .field(Field::output("severity").ty("int").desc("1 is an outage"))
    .field(Field::output("note").optional())
    .build()?;
```

**JSON**, since signatures serialize with serde and validate on the way back
in. `Signature::from_spec` accepts all three forms, plus the name of a
registered signature, which is what the CLIs use.

## Coercion

Models write text, so coercion is forgiving where that is safe and strict where
it is not.

- **Strings are parsed:** `" 42 "` becomes `42` for an integer field, and
  `"yes"` becomes `true` for a boolean.
- **Arrays and objects** may arrive as JSON text.
- **Integral floats are accepted for integer fields, but fractions are
  rejected.** The Ruby gem truncates `3.7` to `3`, which hands back a number the
  model never wrote.
- **NaN and infinity are rejected,** since JSON cannot hold them.
- **A `null` counts as absent.** It takes the field's default, is omitted if the
  field is optional, and is an error otherwise.
- **Undeclared keys are dropped,** and results come back in declaration order.

Type names are resolved when the signature is built. Aliases (`int`, `bool`,
`list`, `hash`, and so on) are replaced with canonical names, so equivalent
signatures produce identical cache keys.

## Swapping pieces

Two registries live here:

| Registry | Built-ins | Use it to |
|---|---|---|
| `codec::CodecPlugin` | `string`, `integer`, `number`, `boolean`, `array`, `object` | add a type, or change how a built-in type coerces |
| `SignaturePlugin` | none | name a signature, or replace a library's signature by name |

Adding a `date` type usable in shorthand strings:

```rust
use cosmos_llm_signature::codec::{CodecPlugin, FieldCodec};
use cosmos_llm_signature::registry::meta;

struct DateCodec;
impl FieldCodec for DateCodec { /* type_hint, json_schema, coerce */ }

cosmos_llm_signature::registry::inventory::submit! {
    CodecPlugin { meta: meta!("date").summary("ISO-8601 date"), codec: &DateCodec }
}

let sig = Signature::parse("invoice -> due: date")?;
```

A plugin crate that is never referenced may be dropped by the linker, so name
it once: `use my_signature_plugins as _;`.

## Command line

```sh
cargo install --path . --features cli

cosmos-llm-signature show 'document -> summary, tags: array[string]'
cosmos-llm-signature show 'q -> a: int' --instructions 'Count.' --json > sig.json
cosmos-llm-signature schema @sig.json | jq .properties
echo '{"a": " 7 "}' | cosmos-llm-signature coerce 'q -> a: int'
cosmos-llm-signature plugins
```

Exit status is 0 on success, 1 when the command fails, and 2 on a usage error.
To get your own plugins into the command line, see "Linking a plugin crate" in
the [registry README](../cosmos-llm-rust-registry/README.md).

## Requirements

Rust 1.75 or later. The `cli` feature is off by default so crates that build on
signatures do not compile an argument parser.

## License

MIT. Enterprise support from [Durable Programming](https://durableprogramming.com).
