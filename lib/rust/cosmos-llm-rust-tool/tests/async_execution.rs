//! End-to-end checks for the async executor, from outside the crate.
//!
//! These assert the verification items of `COSMOS_LLM_PLAN/020.010` — sync and
//! async tools coexisting in one registry and one batch, timeouts not blocking
//! siblings, and the concurrency bound being respected.

#![cfg(feature = "async")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use cosmos_llm_tool::{Executor, ParameterType, Registry, ToolDefinition, ToolError};
use serde_json::json;

/// A synchronous tool: arithmetic, which gains nothing from a future.
fn sync_add() -> ToolDefinition {
    ToolDefinition::new("add")
        .description("Adds two numbers")
        .param("a", ParameterType::Number, true, "")
        .param("b", ParameterType::Number, true, "")
        .handler(|p| {
            let a = p["a"].as_f64().ok_or("a must be a number")?;
            let b = p["b"].as_f64().ok_or("b must be a number")?;
            Ok(json!(a + b))
        })
}

/// An asynchronous tool, standing in for I/O-bound work.
fn async_lookup() -> ToolDefinition {
    ToolDefinition::new("lookup")
        .description("Looks something up")
        .param("key", ParameterType::String, true, "")
        .async_handler(|p| {
            Box::pin(async move {
                tokio::time::sleep(Duration::from_millis(5)).await;
                let key = p["key"].as_str().ok_or("key must be a string")?;
                Ok(json!(format!("value for {key}")))
            })
        })
}

#[tokio::test]
async fn one_registry_holds_both_handler_kinds() {
    let mut registry = Registry::new();
    registry.register(sync_add());
    registry.register(async_lookup());

    assert_eq!(registry.all().len(), 2);
    assert!(!registry.get("add").unwrap().is_async());
    assert!(registry.get("lookup").unwrap().is_async());
}

#[tokio::test]
async fn one_batch_runs_both_handler_kinds() {
    // What an agent loop actually needs: dispatch whatever the model asked for
    // without first sorting the tools by handler kind.
    let mut registry = Registry::new();
    registry.register(sync_add());
    registry.register(async_lookup());

    let calls = vec![
        (registry.get("add").unwrap(), json!({"a": 2.0, "b": 3.0})),
        (registry.get("lookup").unwrap(), json!({"key": "x"})),
        (registry.get("add").unwrap(), json!({"a": 10.0, "b": 1.0})),
    ];

    let results = Executor::execute_all(calls, 4).await;

    assert_eq!(results[0].as_ref().unwrap(), &json!(5.0));
    assert_eq!(results[1].as_ref().unwrap(), &json!("value for x"));
    assert_eq!(results[2].as_ref().unwrap(), &json!(11.0));
}

#[tokio::test]
async fn a_timed_out_tool_does_not_block_its_siblings() {
    let hang = ToolDefinition::new("hang")
        .timeout(Duration::from_millis(20))
        .async_handler(|_| {
            Box::pin(async {
                tokio::time::sleep(Duration::from_secs(30)).await;
                Ok(json!("never"))
            })
        });
    let quick = async_lookup();

    let calls = vec![
        (&hang, json!({})),
        (&quick, json!({"key": "a"})),
        (&quick, json!({"key": "b"})),
    ];

    let results = Executor::execute_all(calls, 4).await;

    assert!(matches!(results[0], Err(ToolError::Timeout { .. })));
    assert_eq!(results[1].as_ref().unwrap(), &json!("value for a"));
    assert_eq!(results[2].as_ref().unwrap(), &json!("value for b"));
}

#[tokio::test]
async fn concurrency_never_exceeds_the_bound() {
    let peak = Arc::new(AtomicUsize::new(0));
    let live = Arc::new(AtomicUsize::new(0));

    let tool = {
        let peak = peak.clone();
        let live = live.clone();
        ToolDefinition::new("tracked").async_handler(move |_| {
            let peak = peak.clone();
            let live = live.clone();
            Box::pin(async move {
                let now = live.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(25)).await;
                live.fetch_sub(1, Ordering::SeqCst);
                Ok(json!(null))
            })
        })
    };

    let calls: Vec<_> = (0..12).map(|_| (&tool, json!({}))).collect();
    let results = Executor::execute_all(calls, 4).await;

    assert_eq!(results.len(), 12);
    assert!(results.iter().all(Result::is_ok));

    let observed = peak.load(Ordering::SeqCst);
    // Bounded above, and genuinely concurrent below: a bound that silently
    // serialised everything would also satisfy `<= 4`.
    assert!(
        observed <= 4,
        "peak concurrency {observed} exceeded the bound"
    );
    assert!(observed > 1, "nothing ran concurrently (peak {observed})");
}

#[tokio::test]
async fn a_sync_tool_still_runs_through_the_sync_path() {
    // The sync API is unchanged; existing callers keep working.
    let tool = sync_add();
    assert_eq!(
        Executor::execute(&tool, &json!({"a": 1.0, "b": 1.0})).unwrap(),
        json!(2.0)
    );
}

#[tokio::test]
async fn an_async_tool_on_the_sync_path_reports_the_fix() {
    let tool = async_lookup();
    let err = Executor::execute(&tool, &json!({"key": "x"})).unwrap_err();
    assert!(err.to_string().contains("execute_async"), "{err}");
}
