//! The command line, driven in-process through `cli::run`.

#![cfg(feature = "cli")]

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use clap::Parser;
use cosmos_llm_cache::cli::{run, Cli};
use serde_json::{json, Value};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A unique directory under the system temp dir, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "cosmos-llm-cache-cli-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        )))
    }

    fn dir(&self) -> &str {
        self.0.to_str().unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn invoke(args: &[&str], stdin: &str) -> Result<String, String> {
    let cli = Cli::try_parse_from(std::iter::once("cosmos-llm-cache").chain(args.iter().copied()))
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    run(cli, &mut stdin.as_bytes(), &mut out).map_err(|e| e.to_string())?;
    Ok(String::from_utf8(out).unwrap())
}

#[test]
fn key_matches_the_library_and_ignores_key_order() {
    let a = invoke(&["key", r#"{"b": 1, "a": 2}"#], "").unwrap();
    let b = invoke(&["key"], r#"{"a": 2, "b": 1}"#).unwrap();
    assert_eq!(a, b);
    assert_eq!(
        a.trim(),
        cosmos_llm_cache::key_for(&json!({ "a": 2, "b": 1 }))
    );
}

#[test]
fn put_then_get_round_trips_a_value() {
    let scratch = Scratch::new();
    invoke(
        &["put", "--dir", scratch.dir(), "k1"],
        r#"{"completion": "hi"}"#,
    )
    .unwrap();
    let out = invoke(&["get", "--dir", scratch.dir(), "k1"], "").unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&out).unwrap(),
        json!({ "completion": "hi" })
    );
}

#[test]
fn get_and_remove_fail_on_a_missing_key() {
    let scratch = Scratch::new();
    assert_eq!(
        invoke(&["get", "--dir", scratch.dir(), "nope"], "").unwrap_err(),
        "no entry for key nope"
    );
    assert!(invoke(&["remove", "--dir", scratch.dir(), "nope"], "").is_err());
}

#[test]
fn clear_requires_confirmation() {
    let scratch = Scratch::new();
    invoke(&["put", "--dir", scratch.dir(), "k1", "1"], "").unwrap();
    let err = invoke(&["clear", "--dir", scratch.dir()], "").unwrap_err();
    assert!(err.starts_with("refusing to delete"), "{err}");
    assert!(invoke(&["get", "--dir", scratch.dir(), "k1"], "").is_ok());

    invoke(&["clear", "--dir", scratch.dir(), "--yes"], "").unwrap();
    assert!(invoke(&["get", "--dir", scratch.dir(), "k1"], "").is_err());
}

#[test]
fn stats_counts_entries() {
    let scratch = Scratch::new();
    for key in ["k1", "k2", "x9"] {
        invoke(&["put", "--dir", scratch.dir(), key, "true"], "").unwrap();
    }
    let out = invoke(&["stats", "--dir", scratch.dir(), "--json"], "").unwrap();
    let stats: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(stats["entries"], 3);
    assert!(stats["bytes"].as_u64().unwrap() > 0);
}

#[test]
fn stats_rejects_a_missing_directory() {
    let scratch = Scratch::new();
    assert!(invoke(&["stats", "--dir", scratch.dir()], "")
        .unwrap_err()
        .ends_with("is not a directory"));
}

#[test]
fn unsafe_keys_and_unknown_stores_are_reported() {
    let scratch = Scratch::new();
    let err = invoke(&["put", "--dir", scratch.dir(), "../escape", "1"], "").unwrap_err();
    assert!(err.starts_with("invalid cache key"), "{err}");
    let err = invoke(
        &["get", "--dir", scratch.dir(), "--store", "redis", "k"],
        "",
    )
    .unwrap_err();
    assert!(err.starts_with("unknown cache store 'redis'"), "{err}");
}

#[test]
fn plugins_lists_the_builtin_stores() {
    let out = invoke(&["plugins", "--json"], "").unwrap();
    let listing: Value = serde_json::from_str(&out).unwrap();
    let names: Vec<&str> = listing["stores"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["disk", "memory", "tiered"]);
}
