//! The command line, driven in-process through `cli::run`.

#![cfg(feature = "cli")]

use std::path::PathBuf;

use clap::Parser;
use cosmos_llm_evaluate::cli::{run, Cli};
use serde_json::Value;

const DEVSET: &str = r#"{"question": "capital of France?", "answer": "Paris"}
{"question": "capital of Spain?", "answer": "Madrid"}
{"question": "capital of Italy?", "answer": "Rome"}
"#;

const PREDICTIONS: &str = r#"{"answer": "paris."}
{"answer": "Barcelona"}
{"answer": "Rome"}
"#;

const SOURCE: &str = "Revenue rose 7% in the third quarter.\nMargins held steady.";

fn invoke(args: &[&str], stdin: &str) -> Result<String, String> {
    let cli =
        Cli::try_parse_from(std::iter::once("cosmos-llm-evaluate").chain(args.iter().copied()))
            .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    run(cli, &mut stdin.as_bytes(), &mut out).map_err(|e| e.to_string())?;
    Ok(String::from_utf8(out).unwrap())
}

/// Writes fixture files into a fresh scratch directory.
fn scratch(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("cosmos-llm-evaluate-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (file, text) in files {
        std::fs::write(dir.join(file), text).unwrap();
    }
    dir
}

fn path(dir: &std::path::Path, file: &str) -> String {
    dir.join(file).to_str().unwrap().to_owned()
}

#[test]
fn score_summarizes_and_shows_the_worst_rows() {
    let dir = scratch(
        "score",
        &[("dev.jsonl", DEVSET), ("preds.jsonl", PREDICTIONS)],
    );
    let out = invoke(
        &[
            "score",
            &path(&dir, "dev.jsonl"),
            &path(&dir, "preds.jsonl"),
            "-m",
            "exact_match",
        ],
        "",
    )
    .unwrap();
    assert!(
        out.starts_with("exact_match on answer: 66.7% (2/3 passed)"),
        "{out}"
    );
    assert!(out.contains("row 2"), "{out}");
    assert!(
        out.contains("expected \"Madrid\"  got \"Barcelona\""),
        "{out}"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn params_are_parsed_as_json_when_possible() {
    let dir = scratch(
        "params",
        &[("dev.jsonl", DEVSET), ("preds.jsonl", PREDICTIONS)],
    );
    let out = invoke(
        &[
            "score",
            &path(&dir, "dev.jsonl"),
            &path(&dir, "preds.jsonl"),
            "-m",
            "exact_match",
            "-p",
            "normalize=false",
            "--json",
        ],
        "",
    )
    .unwrap();
    let report: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(report["passed"], 1, "strict matching rejects 'paris.'");
    assert_eq!(report["rows"].as_array().unwrap().len(), 3);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn fail_under_turns_a_low_score_into_an_error() {
    let dir = scratch(
        "floor",
        &[("dev.jsonl", DEVSET), ("preds.jsonl", PREDICTIONS)],
    );
    let (dev, preds) = (path(&dir, "dev.jsonl"), path(&dir, "preds.jsonl"));
    let err = invoke(
        &["score", &dev, &preds, "-m", "em", "--fail-under", "0.9"],
        "",
    )
    .unwrap_err();
    assert_eq!(err, "score 0.667 is below --fail-under 0.9");
    assert!(invoke(
        &["score", &dev, &preds, "-m", "em", "--fail-under", "0.5"],
        ""
    )
    .is_ok());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn misaligned_predictions_and_bad_options_are_errors() {
    let dir = scratch(
        "bad",
        &[
            ("dev.jsonl", DEVSET),
            ("preds.jsonl", "{\"answer\": \"Paris\"}\n"),
        ],
    );
    let (dev, preds) = (path(&dir, "dev.jsonl"), path(&dir, "preds.jsonl"));
    let err = invoke(&["score", &dev, &preds, "-m", "f1"], "").unwrap_err();
    assert_eq!(err, "devset has 3 examples but there are 1 predictions");
    let err = invoke(&["score", &dev, &preds, "-m", "f1", "-p", "oops"], "").unwrap_err();
    assert!(err.contains("KEY=VALUE"), "{err}");
    let err = invoke(&["score", &dev, &preds, "-m", "bleu"], "").unwrap_err();
    assert!(err.starts_with("unknown metric 'bleu'"), "{err}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn evidence_passes_when_every_quote_is_found() {
    let dir = scratch("evidence-ok", &[("source.txt", SOURCE)]);
    let record =
        r#"{"claims": [{"evidence": "revenue rose 7%"}, {"evidence": "Margins held steady"}]}"#;
    let out = invoke(&["evidence", "--source", &path(&dir, "source.txt")], record).unwrap();
    assert!(out.starts_with("all 2 quotes found in "), "{out}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn evidence_fails_and_names_unverified_quotes() {
    let dir = scratch("evidence-bad", &[("source.txt", SOURCE)]);
    let record = r#"{"claims": [{"q": "revenue rose 7%"}, {"q": "profit doubled"}]}"#;
    let source = path(&dir, "source.txt");
    let err = invoke(&["evidence", "-s", &source, "-k", "q", record], "").unwrap_err();
    assert!(err.starts_with("1 of 2 quotes not found in "), "{err}");

    let mut out = Vec::new();
    let cli = Cli::try_parse_from([
        "cosmos-llm-evaluate",
        "evidence",
        "-s",
        &source,
        "-k",
        "q",
        "--json",
        record,
    ])
    .unwrap();
    assert!(run(cli, &mut "".as_bytes(), &mut out).is_err());
    let result: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        result["unverified"][0],
        "claims[1].q: quote not found in source: \"profit doubled\""
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn plugins_lists_the_builtin_metrics() {
    let out = invoke(&["plugins", "--json"], "").unwrap();
    let listing: Value = serde_json::from_str(&out).unwrap();
    let names: Vec<&str> = listing
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["contains", "exact_match", "f1", "grounded", "set_overlap"]
    );
}
