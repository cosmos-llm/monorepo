//! The command line, driven in-process through `cli::run`.

#![cfg(feature = "cli")]

use clap::Parser;
use cosmos_llm_adapter::cli::{run, Cli};
use serde_json::{json, Value};

fn invoke(args: &[&str], stdin: &str) -> Result<String, String> {
    let cli =
        Cli::try_parse_from(std::iter::once("cosmos-llm-adapter").chain(args.iter().copied()))
            .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    run(cli, &mut stdin.as_bytes(), &mut out).map_err(|e| e.to_string())?;
    Ok(String::from_utf8(out).unwrap())
}

fn scratch(name: &str, contents: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cosmos-llm-adapter-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, contents).unwrap();
    path
}

#[test]
fn format_prints_a_message_array() {
    let out = invoke(
        &["format", "question -> answer", r#"{"question": "2+2?"}"#],
        "",
    )
    .unwrap();
    let messages: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(messages[0]["role"], "system");
    assert_eq!(
        messages[1],
        json!({ "role": "user", "content": "[[ ## question ## ]]\n2+2?\n\n[[ ## done ## ]]" })
    );
}

#[test]
fn format_reads_inputs_from_stdin_and_coerces_them() {
    let out = invoke(
        &["format", "n: int -> a", "--adapter", "json"],
        r#"{"n": "3"}"#,
    )
    .unwrap();
    let messages: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(messages[1]["content"], "{\n  \"n\": 3\n}");
}

#[test]
fn format_with_params_includes_response_format() {
    let out = invoke(
        &[
            "format",
            "q -> a",
            "--adapter",
            "json_schema",
            "--params",
            r#"{"q": "x"}"#,
        ],
        "",
    )
    .unwrap();
    let output: Value = serde_json::from_str(&out).unwrap();
    assert!(output["messages"].is_array());
    assert_eq!(output["params"]["response_format"]["type"], "json_schema");
}

#[test]
fn format_includes_demos_from_a_jsonl_file() {
    let demos = scratch(
        "demos.jsonl",
        "{\"q\": \"1+1\", \"a\": \"2\"}\n\n{\"q\": \"2+2\", \"a\": \"4\"}\n",
    );
    let path = demos.to_str().unwrap();
    let out = invoke(
        &["format", "q -> a", "--demos", path, r#"{"q": "3+3"}"#],
        "",
    )
    .unwrap();
    let messages: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(messages.as_array().unwrap().len(), 6);
    assert_eq!(
        messages[2]["content"],
        "[[ ## a ## ]]\n2\n\n[[ ## done ## ]]"
    );
}

#[test]
fn parse_reads_a_reply_file() {
    let reply = scratch("reply.txt", "[[ ## a ## ]]\n42\n[[ ## done ## ]]\n");
    let out = invoke(&["parse", "q -> a: int", reply.to_str().unwrap()], "").unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&out).unwrap(),
        json!({ "a": 42 })
    );
}

#[test]
fn parse_reads_stdin_and_reports_failures() {
    let out = invoke(
        &["parse", "q -> a: int", "--adapter", "json", "-"],
        "{\"a\": 7}",
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&out).unwrap(),
        json!({ "a": 7 })
    );

    let err = invoke(&["parse", "q -> a"], "no headers").unwrap_err();
    assert_eq!(
        err,
        "Expected field headers like [[ ## a ## ]] but found none in the response."
    );
}

#[test]
fn unknown_adapters_name_the_alternatives() {
    let err = invoke(&["parse", "q -> a", "--adapter", "xml"], "").unwrap_err();
    assert_eq!(
        err,
        "unknown adapter 'xml'; available: chat, json, json_schema"
    );
}

#[test]
fn plugins_lists_the_builtin_adapters() {
    let out = invoke(&["plugins", "--json"], "").unwrap();
    let listing: Value = serde_json::from_str(&out).unwrap();
    let names: Vec<&str> = listing["adapters"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["chat", "json", "json_schema"]);
    assert!(invoke(&["plugins"], "").unwrap().contains("* chat"));
}
