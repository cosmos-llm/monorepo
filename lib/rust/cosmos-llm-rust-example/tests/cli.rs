//! The command line, driven in-process through `cli::run`.

#![cfg(feature = "cli")]

use std::path::PathBuf;

use clap::Parser;
use cosmos_llm_example::cli::{run, Cli};
use serde_json::Value;

const DEVSET: &str = r#"{"question": "2+2?", "answer": "4"}
{"question": "3+3?", "answer": "6"}
{"answer": "8"}
"#;

fn invoke(args: &[&str], stdin: &str) -> Result<String, String> {
    let cli =
        Cli::try_parse_from(std::iter::once("cosmos-llm-example").chain(args.iter().copied()))
            .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    run(cli, &mut stdin.as_bytes(), &mut out).map_err(|e| e.to_string())?;
    Ok(String::from_utf8(out).unwrap())
}

fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("cosmos-llm-example-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn lines(text: &str) -> Vec<Value> {
    text.lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn inspect_reports_rows_missing_inputs() {
    let out = invoke(&["inspect", "-", "--inputs", "question", "--json"], DEVSET).unwrap();
    let summary: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(summary["rows"], 3);
    assert_eq!(summary["keys"]["answer"], 3);
    assert_eq!(summary["missing_inputs"][0]["row"], 3);

    let text = invoke(&["inspect", "-", "--inputs", "question"], DEVSET).unwrap();
    assert!(text.contains("1 rows are missing inputs"), "{text}");
}

#[test]
fn inputs_and_labels_project_columns() {
    let inputs = lines(&invoke(&["inputs", "-", "-i", "question"], DEVSET).unwrap());
    assert_eq!(inputs[0], serde_json::json!({ "question": "2+2?" }));
    let labels = lines(&invoke(&["labels", "-", "-i", "question"], DEVSET).unwrap());
    assert_eq!(labels[1], serde_json::json!({ "answer": "6" }));
    assert!(invoke(&["labels", "-"], DEVSET)
        .unwrap_err()
        .contains("needs --inputs"));
}

#[test]
fn split_writes_both_files_and_refuses_to_overwrite() {
    let dir = scratch("split");
    let train = dir.join("train.jsonl");
    let dev = dir.join("dev.json");
    let (train_s, dev_s) = (train.to_str().unwrap(), dev.to_str().unwrap());

    let out = invoke(
        &[
            "split",
            "-",
            "--train",
            "0.67",
            "--train-out",
            train_s,
            "--dev-out",
            dev_s,
        ],
        DEVSET,
    )
    .unwrap();
    assert!(out.contains("2 training examples"), "{out}");
    assert_eq!(lines(&std::fs::read_to_string(&train).unwrap()).len(), 2);
    // The extension picks the output format.
    let dev_rows: Value = serde_json::from_str(&std::fs::read_to_string(&dev).unwrap()).unwrap();
    assert_eq!(dev_rows.as_array().unwrap().len(), 1);

    let err = invoke(
        &["split", "-", "--train-out", train_s, "--dev-out", dev_s],
        DEVSET,
    )
    .unwrap_err();
    assert!(err.contains("already exists"), "{err}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn convert_changes_format() {
    let out = invoke(
        &["convert", "-", "--format", "json", "--to", "jsonl"],
        r#"[{"a": 1}, {"a": 2}]"#,
    )
    .unwrap();
    assert_eq!(out, "{\"a\":1}\n{\"a\":2}\n");
}

#[test]
fn parse_errors_name_the_line() {
    let err = invoke(&["inputs", "-"], "{}\n[]\n").unwrap_err();
    assert_eq!(err, "line 2: expected a JSON object, found an array");
}

#[test]
fn unknown_formats_list_the_alternatives() {
    let err = invoke(&["convert", "-", "--to", "csv"], "{}").unwrap_err();
    assert_eq!(err, "unknown devset format 'csv'; available: json, jsonl");
}
