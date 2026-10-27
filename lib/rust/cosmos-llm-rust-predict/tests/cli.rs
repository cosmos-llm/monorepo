//! The command line, driven in-process through `cli::run`, against a backend
//! this test registers the same way a plugin crate would.

#![cfg(feature = "cli")]

use clap::Parser;
use cosmos_llm_adapter::Message;
use cosmos_llm_predict::cli::{run, Cli};
use cosmos_llm_predict::registry::meta;
use cosmos_llm_predict::{
    BackendOptions, BackendPlugin, CallOptions, Completion, LanguageModel, PredictError,
};
use futures_util::future::BoxFuture;
use serde_json::{json, Value};

/// Answers 42, or fails when the prompt mentions FAIL.
struct Fixture;

impl LanguageModel for Fixture {
    fn complete<'a>(
        &'a self,
        messages: &'a [Message],
        _: &'a CallOptions,
    ) -> BoxFuture<'a, Result<Completion, PredictError>> {
        let prompt = messages
            .last()
            .map(|m| m.content.clone())
            .unwrap_or_default();
        Box::pin(async move {
            if prompt.contains("FAIL") {
                return Err(PredictError::Model("fixture refused".into()));
            }
            Ok(Completion {
                text: "[[ ## answer ## ]]\n42\n\n[[ ## done ## ]]".to_owned(),
                usage: None,
            })
        })
    }

    fn identity(&self) -> String {
        "fixture".to_owned()
    }
}

fn build(_: &BackendOptions) -> Result<Box<dyn LanguageModel>, PredictError> {
    Ok(Box::new(Fixture))
}

cosmos_llm_predict::registry::inventory::submit! {
    BackendPlugin { meta: meta!("fixture").summary("Test backend"), build }
}

fn invoke(args: &[&str], stdin: &str) -> Result<String, String> {
    let cli =
        Cli::try_parse_from(std::iter::once("cosmos-llm-predict").chain(args.iter().copied()))
            .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    run(cli, &mut stdin.as_bytes(), &mut out).map_err(|e| e.to_string())?;
    Ok(String::from_utf8(out).unwrap())
}

#[test]
fn run_prints_the_coerced_values() {
    let out = invoke(
        &[
            "run",
            "q -> answer: int",
            r#"{"q": "?"}"#,
            "--backend",
            "fixture",
        ],
        "",
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&out).unwrap(),
        json!({ "answer": 42 })
    );
}

#[test]
fn full_output_includes_the_reply_and_bookkeeping() {
    let out = invoke(
        &["run", "q -> answer", "--backend", "fixture", "--full"],
        r#"{"q": "?"}"#,
    )
    .unwrap();
    let full: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(full["values"]["answer"], "42");
    assert_eq!(full["attempts"], 1);
    assert!(full["completion"]
        .as_str()
        .unwrap()
        .contains("[[ ## answer ## ]]"));
}

#[test]
fn jsonl_keeps_rows_aligned_when_some_fail() {
    let stdin = "{\"q\": \"one\"}\n\n{\"q\": \"FAIL\"}\nnot json\n{\"q\": \"four\"}\n";
    let args = [
        "run",
        "q -> answer: int",
        "--jsonl",
        "--backend",
        "fixture",
        "--concurrency",
        "3",
    ];
    let cli = Cli::try_parse_from(std::iter::once("cosmos-llm-predict").chain(args)).unwrap();
    let mut out = Vec::new();
    let err = run(cli, &mut stdin.as_bytes(), &mut out).unwrap_err();
    assert_eq!(err.to_string(), "2 row(s) failed; see the messages above");

    let lines: Vec<Value> = String::from_utf8(out)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        lines,
        [
            json!({ "answer": 42 }),
            json!({}),
            json!({}),
            json!({ "answer": 42 })
        ]
    );
}

#[test]
fn jsonl_rejects_an_inputs_argument() {
    let err = invoke(
        &["run", "q -> a", "{}", "--jsonl", "--backend", "fixture"],
        "",
    )
    .unwrap_err();
    assert!(err.contains("--jsonl reads inputs from stdin"), "{err}");
}

#[test]
fn unknown_plugins_name_the_alternatives() {
    let err = invoke(&["run", "q -> a", "{}", "--backend", "nope"], "").unwrap_err();
    assert!(
        err.starts_with("unknown backend 'nope'; available: cosmos, fixture"),
        "{err}"
    );
    let err = invoke(
        &[
            "run",
            "q -> a",
            "{}",
            "--backend",
            "fixture",
            "--strategy",
            "nope",
        ],
        "",
    )
    .unwrap_err();
    assert!(err.starts_with("unknown strategy 'nope'"), "{err}");
}

#[test]
fn plugins_lists_registered_backends_including_ours() {
    let out = invoke(&["plugins", "--json"], "").unwrap();
    let listing: Value = serde_json::from_str(&out).unwrap();
    let backends: Vec<&str> = listing["backends"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert_eq!(backends, ["cosmos", "fixture"]);
    let text = invoke(&["plugins"], "").unwrap();
    assert!(text.contains("chain_of_thought"), "{text}");
}
