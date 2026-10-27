use serde_json::{Map, Value};

use cosmos_llm_example::Example;
use cosmos_llm_registry::{Meta, Plugin};

use crate::error::EvaluateError;

/// A program's output values, keyed by field name.
pub type Values = Map<String, Value>;

/// Scores one prediction against its labeled example.
///
/// A score is usually in `0.0..=1.0`, where `1.0` is a pass. Scores outside
/// that range are kept as returned, since a custom metric may have its own
/// scale, but [`Report::percent`](crate::Report::percent) and the default
/// pass threshold assume the unit range. A score that is NaN or infinite is
/// never kept: the row is recorded as an error scoring zero, because an
/// average containing NaN is NaN and would hide every other row.
///
/// Any `Fn(&Example, &Values) -> f64` is a metric:
///
/// ```
/// use cosmos_llm_evaluate::{Metric, Values};
/// use cosmos_llm_example::Example;
/// use serde_json::json;
///
/// let non_empty = |_: &Example, prediction: &Values| {
///     let empty = prediction.get("answer").and_then(|v| v.as_str()).map_or(true, str::is_empty);
///     if empty { 0.0 } else { 1.0 }
/// };
///
/// let example = Example::default();
/// let prediction = json!({ "answer": "4" }).as_object().unwrap().clone();
/// assert_eq!(non_empty.score(&example, &prediction).unwrap(), 1.0);
/// ```
pub trait Metric: Send + Sync {
    /// Scores `prediction` against `example`.
    ///
    /// # Errors
    ///
    /// Returns [`EvaluateError::Metric`] when the example or prediction
    /// cannot be scored at all — as opposed to scoring zero.
    fn score(&self, example: &Example, prediction: &Values) -> Result<f64, EvaluateError>;
}

impl<F> Metric for F
where
    F: Fn(&Example, &Values) -> f64 + Send + Sync,
{
    fn score(&self, example: &Example, prediction: &Values) -> Result<f64, EvaluateError> {
        Ok(self(example, prediction))
    }
}

impl Metric for Box<dyn Metric> {
    fn score(&self, example: &Example, prediction: &Values) -> Result<f64, EvaluateError> {
        (**self).score(example, prediction)
    }
}

/// Settings for building a registered metric by name.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricOptions {
    /// The field the metric scores. Defaults to `"answer"`.
    pub field: String,
    /// Metric-specific parameters, e.g. `{"normalize": false}`.
    pub params: Values,
}

impl Default for MetricOptions {
    fn default() -> Self {
        Self::new("answer")
    }
}

impl MetricOptions {
    /// Options scoring `field`, with no parameters.
    pub fn new(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            params: Map::new(),
        }
    }

    /// Adds a parameter.
    #[must_use]
    pub fn param(mut self, name: impl Into<String>, value: impl Into<Value>) -> Self {
        self.params.insert(name.into(), value.into());
        self
    }

    /// Rejects any parameter not in `accepted`.
    ///
    /// Metric builders call this first, so that a misspelled parameter fails
    /// loudly instead of silently leaving the default in place.
    ///
    /// # Errors
    ///
    /// Returns [`EvaluateError::Config`] naming the unknown parameter and the
    /// accepted ones.
    pub fn expect_only(&self, metric: &str, accepted: &[&str]) -> Result<(), EvaluateError> {
        match self
            .params
            .keys()
            .find(|name| !accepted.contains(&name.as_str()))
        {
            None => Ok(()),
            Some(name) if accepted.is_empty() => Err(EvaluateError::Config(format!(
                "metric '{metric}' takes no parameters, got '{name}'"
            ))),
            Some(name) => Err(EvaluateError::Config(format!(
                "metric '{metric}' has no parameter '{name}'; accepted: {}",
                accepted.join(", ")
            ))),
        }
    }

    /// A string parameter, or `default` when absent.
    ///
    /// # Errors
    ///
    /// Returns [`EvaluateError::Config`] when the parameter is not a string.
    pub fn str_param(&self, name: &str, default: &str) -> Result<String, EvaluateError> {
        match self.params.get(name) {
            None => Ok(default.to_owned()),
            Some(Value::String(text)) => Ok(text.clone()),
            Some(other) => Err(EvaluateError::Config(format!(
                "parameter '{name}' must be a string, got {other}"
            ))),
        }
    }

    /// A numeric parameter, or `default` when absent.
    ///
    /// # Errors
    ///
    /// Returns [`EvaluateError::Config`] when the parameter is not a finite
    /// number.
    pub fn f64_param(&self, name: &str, default: f64) -> Result<f64, EvaluateError> {
        match self.params.get(name) {
            None => Ok(default),
            Some(value) => value.as_f64().filter(|n| n.is_finite()).ok_or_else(|| {
                EvaluateError::Config(format!("parameter '{name}' must be a number, got {value}"))
            }),
        }
    }

    /// A boolean parameter, or `default` when absent.
    ///
    /// # Errors
    ///
    /// Returns [`EvaluateError::Config`] when the parameter is not a boolean.
    pub fn bool_param(&self, name: &str, default: bool) -> Result<bool, EvaluateError> {
        match self.params.get(name) {
            None => Ok(default),
            Some(Value::Bool(flag)) => Ok(*flag),
            Some(other) => Err(EvaluateError::Config(format!(
                "parameter '{name}' must be true or false, got {other}"
            ))),
        }
    }
}

/// A named metric registration.
///
/// ```
/// use cosmos_llm_evaluate::{metric, Metric, MetricOptions, MetricPlugin, EvaluateError, Values};
/// use cosmos_llm_evaluate::registry::meta;
/// use cosmos_llm_example::Example;
///
/// /// Passes when the predicted field is no longer than `max` characters.
/// fn brevity(options: &MetricOptions) -> Result<Box<dyn Metric>, EvaluateError> {
///     options.expect_only("brevity", &["max"])?;
///     let field = options.field.clone();
///     let max = options.f64_param("max", 280.0)?;
///     Ok(Box::new(move |_: &Example, prediction: &Values| {
///         let len = prediction.get(&field).and_then(|v| v.as_str()).map_or(0, |s| s.chars().count());
///         if (len as f64) <= max { 1.0 } else { 0.0 }
///     }))
/// }
///
/// cosmos_llm_evaluate::registry::inventory::submit! {
///     MetricPlugin { meta: meta!("brevity").summary("Short enough to post"), build: brevity }
/// }
///
/// let short = metric("brevity", &MetricOptions::new("reply").param("max", 10)).unwrap();
/// let prediction = serde_json::json!({ "reply": "ok" }).as_object().unwrap().clone();
/// assert_eq!(short.score(&Example::default(), &prediction).unwrap(), 1.0);
/// ```
pub struct MetricPlugin {
    /// The metric's name and aliases.
    pub meta: Meta,
    /// Builds the metric from options.
    pub build: fn(&MetricOptions) -> Result<Box<dyn Metric>, EvaluateError>,
}

impl Plugin for MetricPlugin {
    fn meta(&self) -> &Meta {
        &self.meta
    }
}

cosmos_llm_registry::inventory::collect!(MetricPlugin);

/// Builds the metric registered under `name`.
///
/// # Errors
///
/// Returns [`EvaluateError::NotFound`] naming the registered metrics, or the
/// builder's own error for bad options.
pub fn metric(name: &str, options: &MetricOptions) -> Result<Box<dyn Metric>, EvaluateError> {
    let plugin = cosmos_llm_registry::require::<MetricPlugin>("metric", name)?;
    (plugin.build)(options)
}
