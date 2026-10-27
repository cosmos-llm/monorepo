//! Reading, writing, and splitting collections of examples.
//!
//! A devset is a file of examples. The format is a [`DevsetFormat`]
//! registered with [`FormatPlugin`]; JSON Lines and a JSON array are built
//! in, and a crate can add CSV, Parquet, or a database query by registering
//! another, or replace the built-in reader by registering under its name.

use std::path::Path;

use serde_json::{Map, Value};

use cosmos_llm_registry::{meta, Meta, Plugin};

use crate::error::ExampleError;
use crate::example::{kind_of, Example};

/// A file format for devsets.
pub trait DevsetFormat: Send + Sync {
    /// Parses a whole file into rows.
    ///
    /// # Errors
    ///
    /// Returns [`ExampleError::Parse`] or [`ExampleError::NotObject`],
    /// naming the offending row where the format has rows.
    fn read(&self, text: &str) -> Result<Vec<Map<String, Value>>, ExampleError>;

    /// Renders rows as a whole file.
    ///
    /// # Errors
    ///
    /// Returns an error when a row cannot be represented in the format.
    fn write(&self, rows: &[Map<String, Value>]) -> Result<String, ExampleError>;
}

/// A devset format registration.
pub struct FormatPlugin {
    /// The format's name and aliases.
    pub meta: Meta,
    /// File extensions, without the dot, that select this format.
    pub extensions: &'static [&'static str],
    /// The format.
    pub format: &'static dyn DevsetFormat,
}

impl Plugin for FormatPlugin {
    fn meta(&self) -> &Meta {
        &self.meta
    }
}

cosmos_llm_registry::inventory::collect!(FormatPlugin);

/// The format registered under `name`.
///
/// # Errors
///
/// Returns [`ExampleError::NotFound`] naming the registered formats.
pub fn format(name: &str) -> Result<&'static dyn DevsetFormat, ExampleError> {
    Ok(cosmos_llm_registry::require::<FormatPlugin>("devset format", name)?.format)
}

/// The format a path's extension selects, falling back to `jsonl`.
///
/// # Errors
///
/// Returns [`ExampleError::NotFound`] only if `jsonl` itself has been
/// unregistered, which cannot happen through the public API.
pub fn format_for_path(path: &Path) -> Result<&'static dyn DevsetFormat, ExampleError> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase);
    if let Some(extension) = extension {
        for plugin in cosmos_llm_registry::active::<FormatPlugin>() {
            if plugin.extensions.contains(&extension.as_str()) {
                return Ok(plugin.format);
            }
        }
    }
    format("jsonl")
}

/// Parses devset text in the given format, marking `input_keys` as inputs.
///
/// # Errors
///
/// Propagates the format's parse errors.
///
/// # Examples
///
/// ```
/// use cosmos_llm_example::devset;
///
/// let text = "{\"q\": \"2+2?\", \"a\": \"4\"}\n\n{\"q\": \"3+3?\", \"a\": \"6\"}\n";
/// let examples = devset::read(text, devset::format("jsonl").unwrap(), &["q"]).unwrap();
/// assert_eq!(examples.len(), 2);
/// assert_eq!(examples[1].labels()["a"], "6");
/// ```
pub fn read(
    text: &str,
    format: &dyn DevsetFormat,
    input_keys: &[&str],
) -> Result<Vec<Example>, ExampleError> {
    Ok(format
        .read(text)?
        .into_iter()
        .map(|row| Example::new(row).with_inputs(input_keys.iter().copied()))
        .collect())
}

/// Reads a devset file, choosing the format by extension.
///
/// # Errors
///
/// Returns [`ExampleError::Io`] when the file cannot be read, and the
/// format's parse errors.
pub fn load(path: impl AsRef<Path>, input_keys: &[&str]) -> Result<Vec<Example>, ExampleError> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path).map_err(|e| ExampleError::Io {
        path: path.to_owned(),
        message: e.to_string(),
    })?;
    read(&text, format_for_path(path)?, input_keys)
}

/// Renders examples in the given format.
///
/// # Errors
///
/// Propagates the format's write errors.
pub fn write(examples: &[Example], format: &dyn DevsetFormat) -> Result<String, ExampleError> {
    let rows: Vec<Map<String, Value>> = examples.iter().map(|e| e.values().clone()).collect();
    format.write(&rows)
}

/// Splits examples into a training and a held-out set.
///
/// The order is shuffled with a seeded generator before splitting, so the
/// same seed always yields the same split — an experiment can be rerun, and
/// two experiments compared, without either one quietly seeing different
/// data. The training set gets `train_fraction` of the examples, rounded to
/// the nearest whole example.
///
/// # Errors
///
/// Returns [`ExampleError::Config`] when `train_fraction` is outside
/// `0.0..=1.0`.
///
/// # Examples
///
/// ```
/// use cosmos_llm_example::{devset, Example};
///
/// let examples: Vec<Example> = (0..10)
///     .map(|n| Example::from_value(serde_json::json!({ "n": n })).unwrap())
///     .collect();
///
/// let (train, dev) = devset::split(examples.clone(), 0.8, 7).unwrap();
/// assert_eq!((train.len(), dev.len()), (8, 2));
/// assert_eq!(devset::split(examples, 0.8, 7).unwrap().1, dev);
/// ```
pub fn split(
    mut examples: Vec<Example>,
    train_fraction: f64,
    seed: u64,
) -> Result<(Vec<Example>, Vec<Example>), ExampleError> {
    if !(0.0..=1.0).contains(&train_fraction) {
        return Err(ExampleError::Config(format!(
            "train fraction must be between 0 and 1, got {train_fraction}"
        )));
    }
    let mut rng = SplitMix64(seed);
    // Fisher–Yates, back to front.
    for i in (1..examples.len()).rev() {
        let j = (rng.next() % (i as u64 + 1)) as usize;
        examples.swap(i, j);
    }
    let cut = ((examples.len() as f64) * train_fraction).round() as usize;
    let dev = examples.split_off(cut.min(examples.len()));
    Ok((examples, dev))
}

/// SplitMix64: tiny, fast, and stable across platforms and releases, which
/// is what a reproducible split needs and all it needs. Not for anything
/// security-sensitive.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// JSON Lines: one object per line, blank lines ignored.
pub struct JsonLines;

/// A single JSON array of objects.
pub struct JsonArray;

impl DevsetFormat for JsonLines {
    fn read(&self, text: &str) -> Result<Vec<Map<String, Value>>, ExampleError> {
        text.lines()
            .enumerate()
            .filter(|(_, line)| !line.trim().is_empty())
            .map(|(index, line)| row(serde_json::from_str(line), Some(index + 1)))
            .collect()
    }

    fn write(&self, rows: &[Map<String, Value>]) -> Result<String, ExampleError> {
        let mut out = String::new();
        for row in rows {
            out.push_str(&Value::Object(row.clone()).to_string());
            out.push('\n');
        }
        Ok(out)
    }
}

impl DevsetFormat for JsonArray {
    fn read(&self, text: &str) -> Result<Vec<Map<String, Value>>, ExampleError> {
        let value: Value = serde_json::from_str(text).map_err(|e| ExampleError::Parse {
            line: None,
            message: e.to_string(),
        })?;
        let Value::Array(items) = value else {
            return Err(ExampleError::Parse {
                line: None,
                message: format!(
                    "expected a JSON array of objects, found {}",
                    kind_of(&value)
                ),
            });
        };
        items
            .into_iter()
            .enumerate()
            .map(|(index, item)| row(Ok(item), Some(index + 1)))
            .collect()
    }

    fn write(&self, rows: &[Map<String, Value>]) -> Result<String, ExampleError> {
        let array = Value::Array(rows.iter().cloned().map(Value::Object).collect());
        serde_json::to_string_pretty(&array)
            .map(|text| text + "\n")
            .map_err(|e| ExampleError::Parse {
                line: None,
                message: e.to_string(),
            })
    }
}

fn row(
    parsed: Result<Value, serde_json::Error>,
    line: Option<usize>,
) -> Result<Map<String, Value>, ExampleError> {
    match parsed {
        Ok(Value::Object(values)) => Ok(values),
        Ok(other) => Err(ExampleError::NotObject {
            line,
            found: kind_of(&other),
        }),
        Err(e) => Err(ExampleError::Parse {
            line,
            message: e.to_string(),
        }),
    }
}

cosmos_llm_registry::inventory::submit! {
    FormatPlugin {
        meta: meta!("jsonl").builtin().aliases(&["ndjson", "jsonlines"]).summary("One JSON object per line"),
        extensions: &["jsonl", "ndjson"],
        format: &JsonLines,
    }
}

cosmos_llm_registry::inventory::submit! {
    FormatPlugin {
        meta: meta!("json").builtin().summary("A JSON array of objects"),
        extensions: &["json"],
        format: &JsonArray,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn numbered(n: usize) -> Vec<Example> {
        (0..n)
            .map(|i| Example::from_value(json!({ "n": i })).unwrap())
            .collect()
    }

    #[test]
    fn jsonl_errors_name_the_line() {
        let err = JsonLines.read("{\"a\": 1}\n\n[1, 2]\n").unwrap_err();
        assert_eq!(
            err.to_string(),
            "line 3: expected a JSON object, found an array"
        );
        let err = JsonLines.read("{\"a\": 1}\n{oops\n").unwrap_err();
        assert!(err.to_string().starts_with("line 2: "), "{err}");
    }

    #[test]
    fn json_arrays_round_trip() {
        let rows = JsonArray.read(r#"[{"a": 1}, {"b": 2}]"#).unwrap();
        let text = JsonArray.write(&rows).unwrap();
        assert_eq!(JsonArray.read(&text).unwrap(), rows);
        assert!(JsonArray
            .read(r#"{"a": 1}"#)
            .unwrap_err()
            .to_string()
            .contains("found an object"));
    }

    #[test]
    fn formats_are_chosen_by_extension() {
        let rows = format_for_path(Path::new("dev.JSON"))
            .unwrap()
            .read("[{}]")
            .unwrap();
        assert_eq!(rows.len(), 1);
        let rows = format_for_path(Path::new("dev.txt"))
            .unwrap()
            .read("{}\n{}")
            .unwrap();
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn split_is_reproducible_and_seed_dependent() {
        let (a, _) = split(numbered(50), 0.5, 1).unwrap();
        let (b, _) = split(numbered(50), 0.5, 1).unwrap();
        let (c, _) = split(numbered(50), 0.5, 2).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn split_keeps_every_example_exactly_once() {
        let (train, dev) = split(numbered(9), 0.7, 3).unwrap();
        assert_eq!((train.len(), dev.len()), (6, 3));
        let mut seen: Vec<i64> = train
            .iter()
            .chain(&dev)
            .map(|e| e.get("n").unwrap().as_i64().unwrap())
            .collect();
        seen.sort_unstable();
        assert_eq!(seen, (0..9).collect::<Vec<_>>());
    }

    #[test]
    fn split_edges() {
        assert_eq!(split(numbered(4), 0.0, 0).unwrap().0.len(), 0);
        assert_eq!(split(numbered(4), 1.0, 0).unwrap().1.len(), 0);
        assert_eq!(split(Vec::new(), 0.5, 0).unwrap(), (vec![], vec![]));
        assert!(split(numbered(4), 1.5, 0).is_err());
        assert!(split(numbered(4), f64::NAN, 0).is_err());
    }
}
