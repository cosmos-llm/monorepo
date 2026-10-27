# cosmos-llm-example

Labeled examples, and the devset files that hold them.

This is the `Example` class from the Ruby `cosmos-llm-predict` gem, moved into
its own crate. Three crates need it: adapters render examples as
demonstrations, predictors produce values shaped like them, and evaluators
score programs against them. None of those should have to depend on another
just to share a row type.

```rust
use cosmos_llm_example::{devset, Example};

let examples = devset::load("dev.jsonl", &["question"])?;
let (train, dev) = devset::split(examples, 0.8, 42)?;

for example in &dev {
    let inputs = example.inputs();   // what the program receives
    let labels = example.labels();   // what it is scored against
}
```

## Inputs and labels

Which keys are inputs is a property of how a devset is *used*, not of the data,
so it is set with `with_inputs` rather than inferred. Until input keys are set,
`inputs()` returns every value and `labels()` returns none. That makes an
unlabeled example (a demonstration, or a row passed straight to a program) work
without ceremony.

Examples serialize as their plain value object. Input keys are deliberately not
serialized: a JSONL file shared between experiments should not have to agree on
them.

## Splits are reproducible

`split` shuffles with a seeded SplitMix64 before cutting, so the same seed gives
the same split on every platform and every release. An experiment can be
rerun, and two experiments compared, without either one quietly seeing
different data. The generator is not suitable for anything security-sensitive.

## Swapping pieces

Devset file formats are a registry. `devset::FormatPlugin` maps a name and file
extensions to a `DevsetFormat`:

| Name | Extensions | Format |
|---|---|---|
| `jsonl` (aliases `ndjson`, `jsonlines`) | `.jsonl`, `.ndjson` | one object per line; blank lines skipped |
| `json` | `.json` | a JSON array of objects |

Register a CSV reader under a new name, or replace the JSONL reader by
registering under `jsonl`. Unknown extensions fall back to `jsonl`. A plugin
crate that is never referenced may be dropped by the linker, so name it once:
`use my_devset_formats as _;`.

## Command line

```sh
cargo install --path . --features cli

cosmos-llm-example inspect dev.jsonl --inputs question
cosmos-llm-example inputs dev.jsonl -i question,context > inputs.jsonl
cosmos-llm-example labels dev.jsonl -i question
cosmos-llm-example split all.jsonl --train 0.8 --seed 7 --train-out train.jsonl --dev-out dev.jsonl
cat rows.json | cosmos-llm-example convert - --format json --to jsonl
```

`inputs` exists for pipelines: its output is exactly what
`cosmos-llm-predict run --jsonl` reads, and that command's output lines up row
for row with the devset for `cosmos-llm-evaluate score`:

```sh
cosmos-llm-example inputs dev.jsonl -i question \
  | cosmos-llm-predict run 'question -> answer' --jsonl --provider openai --model gpt-5 \
  > preds.jsonl
cosmos-llm-evaluate score dev.jsonl preds.jsonl --metric exact_match
```

`split` refuses to overwrite existing files unless given `--force`. Parse errors
name the line.

## Requirements

Rust 1.75 or later. The `cli` feature is off by default.

## License

MIT. Enterprise support from [Durable Programming](https://durableprogramming.com).
