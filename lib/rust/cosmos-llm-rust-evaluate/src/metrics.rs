//! Ready-made metrics.
//!
//! Each builder returns a metric bound to the field it scores. The five
//! answer-comparison and grounding metrics are also registered by name with
//! [`MetricPlugin`], so a CLI or config file can select them; the two
//! combinators, [`all_of`] and [`average`], take other metrics and are only
//! available from code.
//!
//! A field missing from the example or the prediction, or present as `null`,
//! scores zero. A missing answer is a wrong answer, not an unscorable one.
//!
//! # Examples
//!
//! ```
//! use cosmos_llm_evaluate::{metrics, Metric};
//! use cosmos_llm_example::Example;
//! use serde_json::json;
//!
//! let example = Example::from_value(json!({ "answer": "the Eiffel Tower" })).unwrap();
//! let prediction = json!({ "answer": "Eiffel tower." }).as_object().unwrap().clone();
//!
//! assert_eq!(metrics::exact_match("answer", true).score(&example, &prediction).unwrap(), 1.0);
//! assert_eq!(metrics::exact_match("answer", false).score(&example, &prediction).unwrap(), 0.0);
//! ```

use std::collections::HashMap;

use serde_json::Value;

use cosmos_llm_example::Example;
use cosmos_llm_registry::meta;

use crate::error::EvaluateError;
use crate::evidence;
use crate::metric::{Metric, MetricOptions, MetricPlugin, Values};

/// Words dropped before comparing text, unless nothing else is left.
pub const STOP_WORDS: &[&str] = &["a", "an", "the"];

/// Lowercases, replaces punctuation with spaces, drops articles, and
/// collapses whitespace.
///
/// Punctuation here means any character that is neither alphanumeric nor
/// whitespace, which covers ASCII and Unicode punctuation and symbols alike.
/// Articles are kept when they are the whole text, so `"The"` does not
/// normalize to nothing.
///
/// # Examples
///
/// ```
/// use cosmos_llm_evaluate::metrics::normalize_text;
///
/// assert_eq!(normalize_text("  The Eiffel-Tower! "), "eiffel tower");
/// assert_eq!(normalize_text("The"), "the");
/// ```
pub fn normalize_text(text: &str) -> String {
    let spaced: String = text
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c.is_whitespace() {
                c
            } else {
                ' '
            }
        })
        .collect();
    let words: Vec<&str> = spaced.split_whitespace().collect();
    let kept: Vec<&str> = words
        .iter()
        .copied()
        .filter(|word| !STOP_WORDS.contains(word))
        .collect();
    if kept.is_empty() { words } else { kept }.join(" ")
}

/// The words of [`normalize_text`]'s output.
pub fn tokenize(text: &str) -> Vec<String> {
    normalize_text(text)
        .split(' ')
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Renders a value as comparable text: strings bare, everything else as JSON.
fn text_of(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn present(value: Option<&Value>) -> Option<&Value> {
    value.filter(|value| !value.is_null())
}

/// Scores 1.0 when the field matches its label exactly. See [`exact_match`].
#[derive(Debug, Clone)]
pub struct ExactMatch {
    field: String,
    normalize: bool,
}

/// Scores 1.0 when the field matches its label exactly, else 0.0.
///
/// With `normalize`, both sides go through [`normalize_text`] first, which
/// forgives case, punctuation, and articles — the differences that almost
/// never matter when checking whether a model got the answer right.
pub fn exact_match(field: impl Into<String>, normalize: bool) -> ExactMatch {
    ExactMatch {
        field: field.into(),
        normalize,
    }
}

impl Metric for ExactMatch {
    fn score(&self, example: &Example, prediction: &Values) -> Result<f64, EvaluateError> {
        let (Some(expected), Some(actual)) = (
            present(example.get(&self.field)),
            present(prediction.get(&self.field)),
        ) else {
            return Ok(0.0);
        };
        let (expected, actual) = (text_of(expected), text_of(actual));
        let matched = if self.normalize {
            normalize_text(&expected) == normalize_text(&actual)
        } else {
            expected == actual
        };
        Ok(if matched { 1.0 } else { 0.0 })
    }
}

/// Token-level F1 between the field and its label. See [`f1`].
#[derive(Debug, Clone)]
pub struct F1 {
    field: String,
}

/// Scores the token-level F1 between the field and its label.
///
/// Useful where exact match is too strict: a correct answer phrased
/// differently, or one carrying extra words. Repeated tokens are counted with
/// multiplicity, so `"yes"` against `"yes yes"` is a partial match.
pub fn f1(field: impl Into<String>) -> F1 {
    F1 {
        field: field.into(),
    }
}

impl Metric for F1 {
    fn score(&self, example: &Example, prediction: &Values) -> Result<f64, EvaluateError> {
        let tokens = |value: Option<&Value>| {
            present(value)
                .map(|v| tokenize(&text_of(v)))
                .unwrap_or_default()
        };
        let expected = tokens(example.get(&self.field));
        let actual = tokens(prediction.get(&self.field));
        if expected.is_empty() || actual.is_empty() {
            return Ok(0.0);
        }

        let mut remaining: HashMap<&str, usize> = HashMap::new();
        for token in &actual {
            *remaining.entry(token.as_str()).or_default() += 1;
        }
        let mut overlap = 0usize;
        for token in &expected {
            if let Some(count) = remaining
                .get_mut(token.as_str())
                .filter(|count| **count > 0)
            {
                *count -= 1;
                overlap += 1;
            }
        }
        if overlap == 0 {
            return Ok(0.0);
        }
        let precision = overlap as f64 / actual.len() as f64;
        let recall = overlap as f64 / expected.len() as f64;
        Ok(2.0 * precision * recall / (precision + recall))
    }
}

/// Scores 1.0 when the label appears in the field. See [`contains`].
#[derive(Debug, Clone)]
pub struct Contains {
    field: String,
}

/// Scores 1.0 when the label appears anywhere in the field, after both are
/// normalized with [`normalize_text`].
///
/// A label that normalizes to nothing scores 0.0 rather than matching every
/// prediction, which is how the Ruby gem's empty-string case would otherwise
/// behave.
pub fn contains(field: impl Into<String>) -> Contains {
    Contains {
        field: field.into(),
    }
}

impl Metric for Contains {
    fn score(&self, example: &Example, prediction: &Values) -> Result<f64, EvaluateError> {
        let (Some(expected), Some(actual)) = (
            present(example.get(&self.field)),
            present(prediction.get(&self.field)),
        ) else {
            return Ok(0.0);
        };
        let expected = normalize_text(&text_of(expected));
        if expected.is_empty() {
            return Ok(0.0);
        }
        let found = normalize_text(&text_of(actual)).contains(&expected);
        Ok(if found { 1.0 } else { 0.0 })
    }
}

/// The fraction of expected items found. See [`set_overlap`].
#[derive(Debug, Clone)]
pub struct SetOverlap {
    field: String,
}

/// Scores the fraction of expected list items the prediction found.
///
/// Order is ignored and duplicates collapse, which is what you want for a set
/// of extracted entities or tags. Items are compared after
/// [`normalize_text`]. A scalar is treated as a one-item list, and an empty
/// expectation scores 0.0.
pub fn set_overlap(field: impl Into<String>) -> SetOverlap {
    SetOverlap {
        field: field.into(),
    }
}

fn normalized_items(value: Option<&Value>) -> Vec<String> {
    let items: Vec<&Value> = match present(value) {
        None => Vec::new(),
        Some(Value::Array(items)) => items.iter().collect(),
        Some(other) => vec![other],
    };
    let mut normalized: Vec<String> = Vec::new();
    for item in items {
        let text = normalize_text(&text_of(item));
        if !normalized.contains(&text) {
            normalized.push(text);
        }
    }
    normalized
}

impl Metric for SetOverlap {
    fn score(&self, example: &Example, prediction: &Values) -> Result<f64, EvaluateError> {
        let expected = normalized_items(example.get(&self.field));
        if expected.is_empty() {
            return Ok(0.0);
        }
        let actual = normalized_items(prediction.get(&self.field));
        let found = expected.iter().filter(|item| actual.contains(item)).count();
        Ok(found as f64 / expected.len() as f64)
    }
}

/// The fraction of quotes found in the source. See [`grounded`].
#[derive(Debug, Clone)]
pub struct Grounded {
    field: String,
    source: String,
    key: String,
    empty_score: f64,
}

/// Scores the fraction of a prediction's quotes that appear in the source.
///
/// This is the metric for extraction work: the prediction's `field` holds
/// claims with a quote under `key` attached to each, the example's `source`
/// field holds the document, and the score is how many of those quotes are
/// actually in it. A model that invents support scores low even when its
/// claims sound right, which is the failure no answer-comparison metric
/// detects. See [`evidence`] for what the comparison forgives.
///
/// A prediction carrying no quotes scores `empty_score`: `1.0` says an
/// extraction that correctly found nothing has invented nothing, `0.0` says a
/// missing quote counts against it.
///
/// # Examples
///
/// ```
/// use cosmos_llm_evaluate::{metrics, Metric};
/// use cosmos_llm_example::Example;
/// use serde_json::json;
///
/// let example = Example::from_value(json!({ "document": "Revenue rose 7% in Q3." })).unwrap();
/// let prediction = json!({ "findings": [
///     { "claim": "growth", "evidence": "revenue rose 7%" },
///     { "claim": "profit", "evidence": "margins doubled" },
/// ]}).as_object().unwrap().clone();
///
/// let metric = metrics::grounded("findings", "document", "evidence", 1.0);
/// assert_eq!(metric.score(&example, &prediction).unwrap(), 0.5);
/// ```
pub fn grounded(
    field: impl Into<String>,
    source: impl Into<String>,
    key: impl Into<String>,
    empty_score: f64,
) -> Grounded {
    Grounded {
        field: field.into(),
        source: source.into(),
        key: key.into(),
        empty_score,
    }
}

impl Metric for Grounded {
    fn score(&self, example: &Example, prediction: &Values) -> Result<f64, EvaluateError> {
        let (Some(source), Some(extracted)) = (
            present(example.get(&self.source)),
            present(prediction.get(&self.field)),
        ) else {
            return Ok(0.0);
        };
        let text = text_of(source);
        let quotes = evidence::collect(extracted, &self.key);
        if quotes.is_empty() {
            return Ok(self.empty_score);
        }
        let supported = quotes
            .iter()
            .filter(|(_, quote)| evidence::is_present(quote, &text))
            .count();
        Ok(supported as f64 / quotes.len() as f64)
    }
}

/// Passes only when every metric passes. See [`all_of`].
pub struct AllOf {
    metrics: Vec<Box<dyn Metric>>,
    threshold: f64,
}

/// Scores 1.0 when every metric scores at least `threshold`, else 0.0.
///
/// Stops at the first metric below the threshold.
pub fn all_of(metrics: Vec<Box<dyn Metric>>, threshold: f64) -> AllOf {
    AllOf { metrics, threshold }
}

impl Metric for AllOf {
    fn score(&self, example: &Example, prediction: &Values) -> Result<f64, EvaluateError> {
        for metric in &self.metrics {
            if metric.score(example, prediction)? < self.threshold {
                return Ok(0.0);
            }
        }
        Ok(1.0)
    }
}

/// The weighted mean of several metrics. See [`average`].
pub struct Average {
    metrics: Vec<Box<dyn Metric>>,
    weights: Vec<f64>,
    total: f64,
}

/// Averages several metrics, optionally weighted.
///
/// # Errors
///
/// Returns [`EvaluateError::Config`] when the number of weights differs from
/// the number of metrics, or a weight is negative or not finite. Weights that
/// sum to zero are allowed and score every example 0.0.
///
/// # Examples
///
/// ```
/// use cosmos_llm_evaluate::{metrics, Metric};
/// use cosmos_llm_example::Example;
/// use serde_json::json;
///
/// let example = Example::from_value(json!({ "answer": "Paris" })).unwrap();
/// let prediction = json!({ "answer": "Paris is the capital" }).as_object().unwrap().clone();
///
/// let metric = metrics::average(
///     vec![Box::new(metrics::exact_match("answer", true)), Box::new(metrics::contains("answer"))],
///     Some(vec![3.0, 1.0]),
/// )
/// .unwrap();
/// assert_eq!(metric.score(&example, &prediction).unwrap(), 0.25);
/// ```
pub fn average(
    metrics: Vec<Box<dyn Metric>>,
    weights: Option<Vec<f64>>,
) -> Result<Average, EvaluateError> {
    let weights = weights.unwrap_or_else(|| vec![1.0; metrics.len()]);
    if weights.len() != metrics.len() {
        return Err(EvaluateError::Config(format!(
            "got {} weights for {} metrics",
            weights.len(),
            metrics.len()
        )));
    }
    if let Some(bad) = weights.iter().find(|w| !w.is_finite() || **w < 0.0) {
        return Err(EvaluateError::Config(format!(
            "weights must be finite and non-negative, got {bad}"
        )));
    }
    let total = weights.iter().sum();
    Ok(Average {
        metrics,
        weights,
        total,
    })
}

impl Metric for Average {
    fn score(&self, example: &Example, prediction: &Values) -> Result<f64, EvaluateError> {
        if self.total == 0.0 {
            return Ok(0.0);
        }
        let mut sum = 0.0;
        for (metric, weight) in self.metrics.iter().zip(&self.weights) {
            sum += metric.score(example, prediction)? * weight;
        }
        Ok(sum / self.total)
    }
}

fn build_exact_match(options: &MetricOptions) -> Result<Box<dyn Metric>, EvaluateError> {
    options.expect_only("exact_match", &["normalize"])?;
    Ok(Box::new(exact_match(
        options.field.clone(),
        options.bool_param("normalize", true)?,
    )))
}

fn build_f1(options: &MetricOptions) -> Result<Box<dyn Metric>, EvaluateError> {
    options.expect_only("f1", &[])?;
    Ok(Box::new(f1(options.field.clone())))
}

fn build_contains(options: &MetricOptions) -> Result<Box<dyn Metric>, EvaluateError> {
    options.expect_only("contains", &[])?;
    Ok(Box::new(contains(options.field.clone())))
}

fn build_set_overlap(options: &MetricOptions) -> Result<Box<dyn Metric>, EvaluateError> {
    options.expect_only("set_overlap", &[])?;
    Ok(Box::new(set_overlap(options.field.clone())))
}

fn build_grounded(options: &MetricOptions) -> Result<Box<dyn Metric>, EvaluateError> {
    options.expect_only("grounded", &["source", "key", "empty_score"])?;
    Ok(Box::new(grounded(
        options.field.clone(),
        options.str_param("source", "document")?,
        options.str_param("key", "evidence")?,
        options.f64_param("empty_score", 1.0)?,
    )))
}

cosmos_llm_registry::inventory::submit! {
    MetricPlugin {
        meta: meta!("exact_match").builtin().aliases(&["em", "exact"])
            .summary("1.0 when the field equals its label (params: normalize)"),
        build: build_exact_match,
    }
}

cosmos_llm_registry::inventory::submit! {
    MetricPlugin {
        meta: meta!("f1").builtin().summary("Token-level F1 against the label"),
        build: build_f1,
    }
}

cosmos_llm_registry::inventory::submit! {
    MetricPlugin {
        meta: meta!("contains").builtin().summary("1.0 when the label appears in the field"),
        build: build_contains,
    }
}

cosmos_llm_registry::inventory::submit! {
    MetricPlugin {
        meta: meta!("set_overlap").builtin().summary("Fraction of expected list items found"),
        build: build_set_overlap,
    }
}

cosmos_llm_registry::inventory::submit! {
    MetricPlugin {
        meta: meta!("grounded").builtin()
            .summary("Fraction of quotes found in the source (params: source, key, empty_score)"),
        build: build_grounded,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metric;
    use serde_json::json;

    fn pair(expected: Value, actual: Value) -> (Example, Values) {
        let example = Example::from_value(json!({ "answer": expected })).unwrap();
        let prediction = json!({ "answer": actual }).as_object().unwrap().clone();
        (example, prediction)
    }

    fn score(metric: &dyn Metric, expected: Value, actual: Value) -> f64 {
        let (example, prediction) = pair(expected, actual);
        metric.score(&example, &prediction).unwrap()
    }

    #[test]
    fn exact_match_forgives_case_punctuation_and_articles() {
        let em = exact_match("answer", true);
        assert_eq!(score(&em, json!("Paris"), json!("paris.")), 1.0);
        assert_eq!(
            score(&em, json!("the Eiffel Tower"), json!("Eiffel Tower")),
            1.0
        );
        assert_eq!(score(&em, json!("Paris"), json!("London")), 0.0);
        assert_eq!(
            score(
                &exact_match("answer", false),
                json!("Paris"),
                json!("paris")
            ),
            0.0
        );
    }

    #[test]
    fn exact_match_compares_non_strings_by_their_json() {
        assert_eq!(
            score(&exact_match("answer", true), json!(4), json!("4")),
            1.0
        );
    }

    #[test]
    fn missing_and_null_fields_score_zero() {
        let example = Example::from_value(json!({ "answer": "Paris" })).unwrap();
        let other = json!({ "other": "Paris" }).as_object().unwrap().clone();
        assert_eq!(
            exact_match("answer", true).score(&example, &other).unwrap(),
            0.0
        );
        assert_eq!(
            score(&exact_match("answer", true), json!(null), json!(null)),
            0.0
        );
    }

    #[test]
    fn f1_scores_overlap_with_multiplicity() {
        let metric = f1("answer");
        assert_eq!(
            score(&metric, json!("the quick fox"), json!("quick fox")),
            1.0
        );
        let partial = score(&metric, json!("quick brown fox"), json!("quick fox"));
        assert!((partial - 0.8).abs() < 1e-9, "{partial}");
        assert_eq!(score(&metric, json!("cats"), json!("dogs")), 0.0);
        let repeated = score(&metric, json!("yes yes"), json!("yes"));
        assert!(repeated > 0.0 && repeated < 1.0, "{repeated}");
    }

    #[test]
    fn contains_finds_the_label_and_refuses_empty_labels() {
        let metric = contains("answer");
        assert_eq!(
            score(&metric, json!("Paris"), json!("The capital is Paris.")),
            1.0
        );
        assert_eq!(
            score(&metric, json!("Paris"), json!("The capital is London.")),
            0.0
        );
        assert_eq!(score(&metric, json!("!!"), json!("anything")), 0.0);
    }

    #[test]
    fn set_overlap_ignores_order_and_duplicates() {
        let metric = set_overlap("answer");
        assert_eq!(score(&metric, json!(["a", "b"]), json!(["a", "c"])), 0.5);
        assert_eq!(
            score(&metric, json!(["a", "b"]), json!(["B", "a", "a"])),
            1.0
        );
        assert_eq!(score(&metric, json!([]), json!(["a"])), 0.0);
        assert_eq!(score(&metric, json!("a"), json!(["a"])), 1.0);
    }

    const SOURCE: &str = "They saw their conversion rate increase by 7-12% after the migration.";

    fn grounded_score(metric: &Grounded, extracted: Value) -> f64 {
        let example = Example::from_value(json!({ "document": SOURCE })).unwrap();
        let prediction = json!({ "findings": extracted })
            .as_object()
            .unwrap()
            .clone();
        metric.score(&example, &prediction).unwrap()
    }

    #[test]
    fn grounded_scores_the_supported_fraction() {
        let metric = grounded("findings", "document", "evidence", 1.0);
        let good = json!({ "evidence": "conversion rate increase by 7-12%" });
        let bad = json!({ "evidence": "revenue tripled overnight" });
        assert_eq!(grounded_score(&metric, json!([good.clone()])), 1.0);
        assert_eq!(grounded_score(&metric, json!([good, bad.clone()])), 0.5);
        assert_eq!(grounded_score(&metric, json!([bad])), 0.0);
        let stitched = json!([{ "evidence": "conversion migration 7-12% saw" }]);
        assert_eq!(grounded_score(&metric, stitched), 0.0);
    }

    #[test]
    fn grounded_empty_score_and_custom_key() {
        assert_eq!(
            grounded_score(
                &grounded("findings", "document", "evidence", 1.0),
                json!([])
            ),
            1.0
        );
        assert_eq!(
            grounded_score(
                &grounded("findings", "document", "evidence", 0.0),
                json!([])
            ),
            0.0
        );
        let quote = json!([{ "quote": "conversion rate increase by 7-12%" }]);
        assert_eq!(
            grounded_score(&grounded("findings", "document", "quote", 0.0), quote),
            1.0
        );
    }

    #[test]
    fn grounded_scores_zero_without_a_source() {
        let example = Example::from_value(json!({ "other": "x" })).unwrap();
        let prediction = json!({ "findings": [{ "evidence": "x" }] })
            .as_object()
            .unwrap()
            .clone();
        let metric = grounded("findings", "document", "evidence", 1.0);
        assert_eq!(metric.score(&example, &prediction).unwrap(), 0.0);
    }

    #[test]
    fn all_of_requires_every_metric() {
        let metric = all_of(
            vec![
                Box::new(exact_match("answer", true)),
                Box::new(contains("answer")),
            ],
            1.0,
        );
        assert_eq!(score(&metric, json!("Paris"), json!("Paris")), 1.0);
        assert_eq!(score(&metric, json!("Paris"), json!("London")), 0.0);
    }

    #[test]
    fn average_validates_weights() {
        let one = || -> Vec<Box<dyn Metric>> { vec![Box::new(exact_match("answer", true))] };
        assert!(average(one(), Some(vec![1.0, 2.0])).is_err());
        assert!(average(one(), Some(vec![-1.0])).is_err());
        assert!(average(one(), Some(vec![f64::NAN])).is_err());
        let zero = average(one(), Some(vec![0.0])).unwrap();
        assert_eq!(score(&zero, json!("a"), json!("a")), 0.0);
        let plain = average(
            vec![
                Box::new(exact_match("answer", true)),
                Box::new(contains("answer")),
            ],
            None,
        )
        .unwrap();
        assert_eq!(
            score(&plain, json!("Paris"), json!("Paris is the capital")),
            0.5
        );
    }

    #[test]
    fn registered_metrics_build_from_options() {
        let em = metric("EM", &MetricOptions::default().param("normalize", false)).unwrap();
        assert_eq!(score(&*em, json!("Paris"), json!("paris")), 0.0);
        let err = metric("f1", &MetricOptions::default().param("normalise", true))
            .err()
            .unwrap();
        assert_eq!(
            err.to_string(),
            "metric 'f1' takes no parameters, got 'normalise'"
        );
        let err = metric("grounded", &MetricOptions::default().param("sorce", "doc"))
            .err()
            .unwrap();
        assert!(
            err.to_string()
                .contains("accepted: source, key, empty_score"),
            "{err}"
        );
        let err = metric(
            "exact_match",
            &MetricOptions::default().param("normalize", "no"),
        )
        .err()
        .unwrap();
        assert!(err.to_string().contains("must be true or false"), "{err}");
        let err = metric("bleu", &MetricOptions::default()).err().unwrap();
        assert!(
            err.to_string()
                .starts_with("unknown metric 'bleu'; available: "),
            "{err}"
        );
    }
}
