//! The command line, driven in-process through `cli::run`.

#![cfg(feature = "cli")]

use clap::Parser;
use cosmos_llm_signature::cli::{run, Cli};
use serde_json::{json, Value};

fn invoke(args: &[&str], stdin: &str) -> Result<String, String> {
    let cli =
        Cli::try_parse_from(std::iter::once("cosmos-llm-signature").chain(args.iter().copied()))
            .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    run(cli, &mut stdin.as_bytes(), &mut out).map_err(|e| e.to_string())?;
    Ok(String::from_utf8(out).unwrap())
}

#[test]
fn show_describes_fields_with_type_hints() {
    let out = invoke(&["show", "doc -> tags: array[string]"], "").unwrap();
    assert!(out.starts_with("doc -> tags\n"), "{out}");
    assert!(out.contains("Instructions (generated):"), "{out}");
    assert!(out.contains("tags  a JSON array of strings"), "{out}");
}

#[test]
fn show_json_round_trips_through_a_file_spec() {
    let out = invoke(
        &["show", "q -> a: int", "--instructions", "Count.", "--json"],
        "",
    )
    .unwrap();
    let dir = std::env::temp_dir().join(format!("cosmos-llm-signature-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("sig.json");
    std::fs::write(&path, &out).unwrap();

    let spec = format!("@{}", path.display());
    let shown = invoke(&["show", &spec], "").unwrap();
    assert!(shown.contains("Count."), "{shown}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn schema_prints_the_output_schema() {
    let out = invoke(&["schema", "q -> a: int"], "").unwrap();
    let schema: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(schema["properties"]["a"], json!({ "type": "integer" }));
}

#[test]
fn coerce_reads_values_from_stdin() {
    let out = invoke(
        &["coerce", "q -> a: int, ok: bool"],
        r#"{"a": " 7 ", "ok": "yes"}"#,
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&out).unwrap(),
        json!({ "a": 7, "ok": true })
    );
}

#[test]
fn coerce_inputs_uses_the_input_fields() {
    let out = invoke(&["coerce", "n: int -> a", "--inputs", r#"{"n": "3"}"#], "").unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&out).unwrap(),
        json!({ "n": 3 })
    );
}

#[test]
fn coercion_failures_are_reported_as_errors() {
    let err = invoke(&["coerce", "q -> a: int", r#"{"a": "lots"}"#], "").unwrap_err();
    assert_eq!(err, "field 'a' expected an integer, got \"lots\"");
    let err = invoke(&["coerce", "q -> a", "not json"], "").unwrap_err();
    assert!(err.starts_with("values are not valid JSON"), "{err}");
}

#[test]
fn plugins_lists_the_builtin_types() {
    let out = invoke(&["plugins", "--json"], "").unwrap();
    let listing: Value = serde_json::from_str(&out).unwrap();
    let names: Vec<&str> = listing["types"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["array", "boolean", "integer", "number", "object", "string"]
    );
}
