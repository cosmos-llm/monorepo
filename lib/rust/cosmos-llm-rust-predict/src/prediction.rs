use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use cosmos_llm_example::Example;

use crate::error::PredictError;
use crate::model::Usage;

/// The typed outputs of one predictor run, and how they were produced.
///
/// [`Prediction::values`] holds exactly the signature's output fields,
/// already coerced. Everything else — the raw reply, usage, whether it came
/// from the cache — is kept alongside rather than mixed in, so a metric that
/// compares outputs to labels never trips over bookkeeping.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Prediction {
    /// The coerced output values, in signature order.
    pub values: Map<String, Value>,
    /// The final reply text, verbatim.
    pub completion: String,
    /// Usage summed across every attempt that reached the model, including
    /// parse retries. Replies served from the cache cost nothing and add
    /// nothing, so a fully cached prediction has `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// Whether the final reply was served from the cache.
    #[serde(default)]
    pub cached: bool,
    /// How many replies were requested, including correction turns.
    #[serde(default)]
    pub attempts: usize,
    /// Notes added by wrapping modules, such as best-of-N's winning score.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub meta: Map<String, Value>,
}

impl Prediction {
    /// The value of an output field, if present.
    pub fn get(&self, field: &str) -> Option<&Value> {
        self.values.get(field)
    }

    /// The value of an output field.
    ///
    /// # Errors
    ///
    /// Returns [`PredictError::Config`] naming the fields that do exist.
    pub fn fetch(&self, field: &str) -> Result<&Value, PredictError> {
        self.values.get(field).ok_or_else(|| {
            let have: Vec<&str> = self.values.keys().map(String::as_str).collect();
            PredictError::Config(format!(
                "prediction has no field '{field}' (have: {})",
                have.join(", ")
            ))
        })
    }

    /// The output values deserialized into a type of your own.
    ///
    /// # Errors
    ///
    /// Returns [`PredictError::Config`] when the values do not fit `T`.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm_predict::Prediction;
    /// use serde::Deserialize;
    ///
    /// #[derive(Deserialize)]
    /// struct Answer { answer: String, confidence: f64 }
    ///
    /// let mut prediction = Prediction::default();
    /// prediction.values.insert("answer".into(), "4".into());
    /// prediction.values.insert("confidence".into(), 0.9.into());
    ///
    /// let typed: Answer = prediction.deserialize().unwrap();
    /// assert_eq!(typed.answer, "4");
    /// ```
    pub fn deserialize<T: DeserializeOwned>(&self) -> Result<T, PredictError> {
        serde_json::from_value(Value::Object(self.values.clone())).map_err(|e| {
            PredictError::Config(format!("prediction does not fit the requested type: {e}"))
        })
    }

    /// The output values as an [`Example`], for use as a demonstration.
    pub fn to_example(&self) -> Example {
        Example::new(self.values.clone())
    }

    /// Consumes the prediction, returning its output values.
    pub fn into_values(self) -> Map<String, Value> {
        self.values
    }
}
