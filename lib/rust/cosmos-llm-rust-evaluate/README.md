# cosmos-llm-evaluate

Score LLM programs against labeled devsets, and check that quoted evidence
actually appears in its source.

An evaluation answers one question: over these examples, how often does this
program get it right? That number is what turns a prompt change or a model
swap into an experiment instead of a guess. This crate computes it, keeps
every row behind it so you can see what to fix, and stays out of the way of
however you built the program.

```rust
use cosmos_llm_evaluate::{metrics, Evaluator, ProgramError, Values};
use cosmos_llm_example::devset;

let dev = devset::load("dev.jsonl", &["question"])?;

let program = |inputs: Values| async move {
    // Call a predictor, a pipeline, a lookup table — anything async.
    answer(inputs).await
};

let report = Evaluator::new(metrics::f1("answer"))
    .concurrency(8)
    .max_errors(Some(5))
    .run(&program, &dev)
    .await?;

println!("{}", report.summary());          // 72.0% (18/25 passed, 1 errored)
for row in report.failures(1.0).iter().take(5) {
    println!("row {}: {:?}", row.index, row.prediction);
}
```

Mirrors the Ruby `cosmos-llm-evaluate` gem.

## What is in it

- **`Program`** — any async function from input values to output values. The
  evaluator does not know or care how a program is built, so this crate does
  not depend on the predictor, adapter, or client crates.
- **`Metric`** — any function from an example and a prediction to a score.
  Built in: `exact_match`, `f1`, `contains`, `set_overlap`, and `grounded`,
  plus the `all_of` and `average` combinators.
- **`Evaluator`** — runs a program over a devset concurrently and scores it.
- **`score_predictions`** — scores predictions produced somewhere else.
- **`evidence`** — verifies quotes against a source document.

## Why it behaves the way it does

**One bad row does not lose the run.** A program or metric that fails on an
example is recorded as a zero-scoring row with its error, and evaluation
continues. Errors count as zero in the average rather than being dropped: a
program that crashes on a third of the devset has a real problem, and
averaging over the survivors would hide it.

**`max_errors` stops early.** Once more than the limit have failed, the run
stops and returns `TooManyErrors` with the first failure's message. The Ruby
gem finishes the sweep first; here every further call would be money spent on
a result that is about to be discarded.

**Rows stay in devset order.** Examples finish in whatever order the model
answers them, but `report.rows()[i]` is always `devset[i]`.

**Non-finite scores are errors.** A metric returning NaN or infinity marks its
row as an error scoring zero, because one NaN would make the whole average
NaN. Finite scores outside `0.0..=1.0` are kept as returned — a custom metric
may have its own scale — but `percent()` and the default pass threshold of
`1.0` assume the unit range.

**Missing means wrong.** A field absent from the example or the prediction, or
`null`, scores zero.

**No runtime is chosen for you.** The evaluator uses `futures-util` to poll
examples concurrently on whatever executor awaits it.

## Grounding

When a model extracts claims and quotes the text supporting each, the
`grounded` metric scores how many of those quotes are really in the document:

```rust
let metric = metrics::grounded("findings", "document", "evidence", 1.0);
```

The check is a contiguous substring match, not a similarity score. A claim
stitched together from words scattered through the document is exactly the
invention it exists to catch. What it does forgive is what text conversion
rewrites: Unicode compatibility forms (NFKC), curly quotes, long dashes, line
wrapping, and case. A prediction with no quotes scores `empty_score`, so you
decide whether "found nothing" is a pass.

## Adding or replacing a metric

Metrics are registered by name through
[`inventory`](https://docs.rs/inventory), which is what the CLI's `--metric`
and any config file resolve against. Register a new name to add one:

```rust
use cosmos_llm_evaluate::{EvaluateError, Metric, MetricOptions, MetricPlugin, Values};
use cosmos_llm_evaluate::registry::meta;
use cosmos_llm_example::Example;

fn brevity(options: &MetricOptions) -> Result<Box<dyn Metric>, EvaluateError> {
    options.expect_only("brevity", &["max"])?;
    let field = options.field.clone();
    let max = options.f64_param("max", 280.0)?;
    Ok(Box::new(move |_: &Example, prediction: &Values| {
        let len = prediction.get(&field).and_then(|v| v.as_str()).map_or(0, str::len);
        if (len as f64) <= max { 1.0 } else { 0.0 }
    }))
}

cosmos_llm_evaluate::registry::inventory::submit! {
    MetricPlugin { meta: meta!("brevity").summary("Short enough to post"), build: brevity }
}
```

Register under a built-in's name — `f1`, say — and yours replaces it. Built-ins
use the lowest priority, so no other configuration is needed. If two plugins
claim a name at the same priority, `cosmos-llm-evaluate plugins` warns about it.

`inventory` collects registrations from every crate linked into the binary. A
plugin crate that is a dependency but never referenced from code can be
dropped by the linker, so name it once:

```rust
use my_metrics as _;
```

To get the command line with your metrics in it, make a two-line binary:

```rust
use my_metrics as _;

fn main() -> std::process::ExitCode {
    cosmos_llm_evaluate::cli::main()
}
```

## Command line

Built with the `cli` feature:

```sh
cargo install --path . --features cli
```

`score` is the offline half of an evaluation. Produce predictions however you
like, one JSON object per devset row and in the same order, then score them:

```sh
cosmos-llm-example inputs dev.jsonl --inputs question \
  | my-program > preds.jsonl \
  && cosmos-llm-evaluate score dev.jsonl preds.jsonl --metric f1
```

```text
f1 on answer: 72.0% (18/25 passed)

Worst 5 of 7 below 1:
  row 4    score 0.00  expected "Madrid"  got "Barcelona"
  ...
```

Useful flags: `--param key=value` (repeatable; values parse as JSON when they
can), `--threshold`, `--show-failures N`, `--json` for the full report, and
`--fail-under 0.9` to make CI fail on a regression.

```sh
cosmos-llm-evaluate score dev.jsonl preds.jsonl --metric exact_match --param normalize=false
cosmos-llm-evaluate score dev.jsonl preds.jsonl --metric grounded --field findings \
    --param source=document --param empty_score=0
cosmos-llm-evaluate evidence --source report.txt finding.json   # exits 1 if any quote is missing
cosmos-llm-evaluate plugins
```

Exit codes: 0 on success, 1 when the command fails (including a score under
`--fail-under` or an unverified quote), 2 on a usage error.

## Requirements

Rust 1.75 or later. The `cli` feature caps `clap` below 4.6, which requires
Rust 1.85.

## Development

```sh
cargo test
cargo test --features cli
cargo clippy --all-targets --features cli -- -D warnings
```

## License

MIT. Enterprise support from [Durable Programming](https://durableprogramming.com).
