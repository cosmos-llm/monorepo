# Changelog

## Unreleased

### Added

- Initial release, porting the Ruby `cosmos-llm-evaluate` gem.
- `Program` trait for anything that maps input values to output values
  asynchronously, with a blanket impl for async closures.
- `Metric` trait, with a blanket impl for `Fn(&Example, &Values) -> f64`.
- Built-in metrics `exact_match`, `f1`, `contains`, `set_overlap`, and
  `grounded`, registered by name through `MetricPlugin`, plus the `all_of`
  and `average` combinators.
- `MetricOptions` for building registered metrics from a field and
  parameters; unknown parameters are rejected rather than ignored.
- `Evaluator`, which runs a program over a devset with bounded concurrency,
  records per-example failures as zero-scoring rows, reports progress, and
  stops early once `max_errors` is exceeded. Runtime-agnostic.
- `score_predictions` for scoring predictions produced elsewhere.
- `Report` and `Row`: mean score, passes, worst-first failures, errors, a
  one-line summary, and JSON serialization.
- `evidence` module: contiguous, NFKC- and punctuation-normalized,
  case-insensitive quote verification against a source document.
- `cosmos-llm-evaluate` CLI (behind the `cli` feature) with `score`,
  `evidence`, and `plugins` subcommands.

### Differences from the Ruby gem

- `max_errors` stops the run as soon as it is exceeded instead of finishing
  the sweep first.
- A metric returning NaN or infinity marks its row as an error.
- `contains` scores 0.0 when the label normalizes to an empty string.
- `average` rejects negative or non-finite weights.
