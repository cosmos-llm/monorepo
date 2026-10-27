use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::error::ExampleError;

/// A set of named values, some of which are marked as inputs.
///
/// An example is one row of a devset: the values a program receives and the
/// labels it is scored against, held together. Which keys are inputs is not
/// a property of the data but of how it is used, so it is set with
/// [`Example::with_inputs`] rather than inferred.
///
/// Until input keys are set, [`Example::inputs`] returns every value and
/// [`Example::labels`] returns none. That makes an unlabeled example — a
/// demonstration, or a row passed straight to a program — work without
/// ceremony.
///
/// Examples serialize as their plain value object. The input keys are not
/// serialized: they belong to the caller's use of the devset, and a JSONL
/// file shared between experiments should not have to agree on them.
///
/// # Examples
///
/// ```
/// use cosmos_llm_example::Example;
/// use serde_json::json;
///
/// let example = Example::from_value(json!({ "question": "2+2?", "answer": "4" }))
///     .unwrap()
///     .with_inputs(["question"]);
///
/// assert_eq!(example.inputs()["question"], "2+2?");
/// assert_eq!(example.labels()["answer"], "4");
/// ```
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Example {
    values: Map<String, Value>,
    input_keys: Vec<String>,
}

impl Example {
    /// Wraps a map of values, with no input keys set.
    pub fn new(values: Map<String, Value>) -> Self {
        Self {
            values,
            input_keys: Vec::new(),
        }
    }

    /// Wraps a JSON object.
    ///
    /// # Errors
    ///
    /// Returns [`ExampleError::NotObject`] for anything but an object.
    pub fn from_value(value: Value) -> Result<Self, ExampleError> {
        match value {
            Value::Object(values) => Ok(Self::new(values)),
            other => Err(ExampleError::NotObject {
                line: None,
                found: kind_of(&other),
            }),
        }
    }

    /// Marks which keys are inputs; the rest become labels.
    #[must_use]
    pub fn with_inputs<I, S>(mut self, keys: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.input_keys = keys.into_iter().map(Into::into).collect();
        self
    }

    /// A copy with `updates` merged over the existing values.
    #[must_use]
    pub fn merge(mut self, updates: Map<String, Value>) -> Self {
        for (key, value) in updates {
            self.values.insert(key, value);
        }
        self
    }

    /// A copy without the given keys, which are also dropped from the inputs.
    #[must_use]
    pub fn without<I, S>(mut self, keys: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for key in keys {
            let key = key.as_ref();
            self.values.shift_remove(key);
            self.input_keys.retain(|input| input != key);
        }
        self
    }

    /// Sets one value.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<Value>) {
        self.values.insert(key.into(), value.into());
    }

    /// The value for `key`, if present.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    /// Whether `key` has a value.
    pub fn contains(&self, key: &str) -> bool {
        self.values.contains_key(key)
    }

    /// Every value.
    pub fn values(&self) -> &Map<String, Value> {
        &self.values
    }

    /// The keys marked as inputs, in the order they were given.
    pub fn input_keys(&self) -> &[String] {
        &self.input_keys
    }

    /// The input values, or every value when no input keys are set.
    ///
    /// Input keys with no value are skipped rather than reported; use
    /// [`Example::missing_inputs`] to find them.
    pub fn inputs(&self) -> Map<String, Value> {
        if self.input_keys.is_empty() {
            return self.values.clone();
        }
        self.input_keys
            .iter()
            .filter_map(|key| {
                self.values
                    .get(key)
                    .map(|value| (key.clone(), value.clone()))
            })
            .collect()
    }

    /// The label values: everything not marked as an input. Empty when no
    /// input keys are set.
    pub fn labels(&self) -> Map<String, Value> {
        if self.input_keys.is_empty() {
            return Map::new();
        }
        self.values
            .iter()
            .filter(|(key, _)| !self.input_keys.contains(key))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }

    /// Input keys that have no value in this example.
    pub fn missing_inputs(&self) -> Vec<&str> {
        self.input_keys
            .iter()
            .filter(|key| !self.values.contains_key(*key))
            .map(String::as_str)
            .collect()
    }

    /// Consumes the example, returning its values.
    pub fn into_values(self) -> Map<String, Value> {
        self.values
    }
}

impl From<Map<String, Value>> for Example {
    fn from(values: Map<String, Value>) -> Self {
        Self::new(values)
    }
}

impl Serialize for Example {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.values.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Example {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Map::deserialize(deserializer).map(Self::new)
    }
}

impl fmt::Display for Example {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", Value::Object(self.values.clone()))
    }
}

/// Names a JSON value's kind for error messages.
pub(crate) fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn qa() -> Example {
        Example::from_value(json!({ "question": "q", "context": "c", "answer": "a" })).unwrap()
    }

    #[test]
    fn without_input_keys_everything_is_an_input() {
        let example = qa();
        assert_eq!(example.inputs().len(), 3);
        assert!(example.labels().is_empty());
    }

    #[test]
    fn input_keys_split_inputs_from_labels_in_order() {
        let example = qa().with_inputs(["context", "question"]);
        assert_eq!(
            example.inputs().keys().collect::<Vec<_>>(),
            ["context", "question"]
        );
        assert_eq!(Value::Object(example.labels()), json!({ "answer": "a" }));
    }

    #[test]
    fn missing_inputs_are_reported_not_invented() {
        let example = qa().with_inputs(["question", "documents"]);
        assert_eq!(example.missing_inputs(), ["documents"]);
        assert_eq!(example.inputs().len(), 1);
    }

    #[test]
    fn without_drops_values_and_input_keys() {
        let example = qa()
            .with_inputs(["question", "context"])
            .without(["context"]);
        assert_eq!(example.input_keys(), ["question"]);
        assert!(!example.contains("context"));
    }

    #[test]
    fn merge_overwrites_and_keeps_input_keys() {
        let mut updates = Map::new();
        updates.insert("answer".to_owned(), json!("b"));
        let example = qa().with_inputs(["question"]).merge(updates);
        assert_eq!(example.get("answer"), Some(&json!("b")));
        assert_eq!(example.input_keys(), ["question"]);
    }

    #[test]
    fn serializes_as_the_plain_value_object() {
        let example = qa().with_inputs(["question"]);
        let text = serde_json::to_string(&example).unwrap();
        assert_eq!(text, r#"{"question":"q","context":"c","answer":"a"}"#);
        let back: Example = serde_json::from_str(&text).unwrap();
        assert_eq!(back, qa());
    }

    #[test]
    fn non_objects_are_rejected() {
        let err = Example::from_value(json!([1])).unwrap_err();
        assert_eq!(err.to_string(), "expected a JSON object, found an array");
    }
}
