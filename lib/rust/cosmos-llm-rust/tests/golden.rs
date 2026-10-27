//! Golden files for the IR types: each has a JSON and a TrongoScript form
//! under `tests/golden/`, and both must load into the same value.
//!
//! Regenerate the `.trongo` files from the `.json` ones with
//! `UPDATE_GOLDEN=1 cargo test --test golden`.

#![cfg(feature = "trongo")]

use std::fmt::Debug;
use std::fs;
use std::path::PathBuf;

use cosmos_llm::{
    from_trongo, to_trongo, CompletionRequest, Message, ModelInfo, Reasoning, StreamChunk,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

fn path(name: &str, ext: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.{ext}"))
}

/// Checks one fixture: JSON and trongo load to the same value, the value
/// serializes back to the JSON, and the trongo file is exactly the dump.
fn check<T>(name: &str)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    let json_text = fs::read_to_string(path(name, "json")).unwrap();
    let json: Value = serde_json::from_str(&json_text).unwrap();
    let from_json: T = serde_json::from_value(json.clone()).unwrap();

    // The serde shape is the contract: serializing gives the fixture back.
    assert_eq!(
        serde_json::to_value(&from_json).unwrap(),
        json,
        "{name}: json round trip"
    );

    let dump = to_trongo(&from_json).unwrap();
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        fs::write(path(name, "trongo"), &dump).unwrap();
    }

    let trongo_text = fs::read_to_string(path(name, "trongo")).unwrap();
    let from_trongo_file: T = from_trongo(&trongo_text).unwrap();
    assert_eq!(
        from_json, from_trongo_file,
        "{name}: trongo file differs from json"
    );
    assert_eq!(
        trongo_text, dump,
        "{name}: trongo file is stale; rerun with UPDATE_GOLDEN=1"
    );

    let reloaded: T = from_trongo(&dump).unwrap();
    assert_eq!(from_json, reloaded, "{name}: dump does not round trip");
}

#[test]
fn message_with_every_block_kind() {
    check::<Message>("message_blocks");
}

#[test]
fn message_with_a_tool_result() {
    check::<Message>("message_tool_result");
}

#[test]
fn stream_chunk_with_thinking() {
    check::<StreamChunk>("stream_chunk_thinking");
}

#[test]
fn reasoning_effort_and_budget() {
    check::<Reasoning>("reasoning");
    check::<Reasoning>("reasoning_budget");
}

#[test]
fn model_info() {
    check::<ModelInfo>("model_info");
}

#[test]
fn completion_request() {
    check::<CompletionRequestEq>("completion_request");
}

/// `CompletionRequest` holds a cancellation token, which has no equality, so
/// compare through its serialized form.
#[derive(Debug, Serialize, serde::Deserialize)]
#[serde(transparent)]
struct CompletionRequestEq(CompletionRequest);

impl PartialEq for CompletionRequestEq {
    fn eq(&self, other: &Self) -> bool {
        serde_json::to_value(&self.0).unwrap() == serde_json::to_value(&other.0).unwrap()
    }
}
