//! The evaluator from outside the crate: ordering, failure handling, and the
//! concurrency bound.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cosmos_llm_evaluate::{metrics, EvaluateError, Evaluator, Metric, ProgramError, Values};
use cosmos_llm_example::Example;
use serde_json::{json, Value};

fn values(value: Value) -> Values {
    value.as_object().unwrap().clone()
}

/// Questions 0..n whose correct answer is the question number doubled.
fn devset(n: usize) -> Vec<Example> {
    (0..n)
        .map(|i| {
            Example::from_value(json!({ "q": i, "context": "c", "answer": (i * 2).to_string() }))
                .unwrap()
                .with_inputs(["q"])
        })
        .collect()
}

fn q(inputs: &Values) -> u64 {
    inputs["q"].as_u64().unwrap()
}

/// Doubles the question, finishing later questions sooner so completion
/// order is the reverse of devset order.
async fn doubler(inputs: Values) -> Result<Values, ProgramError> {
    let n = q(&inputs);
    tokio::time::sleep(Duration::from_millis(20u64.saturating_sub(n * 4))).await;
    Ok(values(json!({ "answer": (n * 2).to_string() })))
}

#[tokio::test]
async fn scores_the_devset_and_keeps_devset_order() {
    let report = Evaluator::new(metrics::exact_match("answer", true))
        .concurrency(5)
        .run(&doubler, &devset(5))
        .await
        .unwrap();
    assert_eq!(report.score(), 1.0);
    let order: Vec<usize> = report.rows().iter().map(|row| row.index).collect();
    assert_eq!(order, [0, 1, 2, 3, 4]);
}

#[tokio::test]
async fn the_program_sees_only_the_inputs() {
    let seen = Mutex::new(Vec::new());
    let program = |inputs: Values| {
        seen.lock().unwrap().push(inputs.clone());
        async { Ok::<_, ProgramError>(values(json!({ "answer": "0" }))) }
    };
    Evaluator::new(metrics::exact_match("answer", true))
        .run_with(program, &devset(1))
        .await
        .unwrap();
    assert_eq!(*seen.lock().unwrap(), [values(json!({ "q": 0 }))]);
}

#[tokio::test]
async fn a_failing_program_scores_zero_without_aborting() {
    let program = |inputs: Values| async move {
        if q(&inputs) == 1 {
            return Err::<Values, ProgramError>("model timed out".into());
        }
        doubler(inputs).await
    };
    let report = Evaluator::new(metrics::exact_match("answer", true))
        .run(&program, &devset(3))
        .await
        .unwrap();
    assert_eq!(report.len(), 3);
    assert_eq!(report.errors().len(), 1);
    let failed = &report.rows()[1];
    assert_eq!(
        (failed.score, failed.error.as_deref()),
        (0.0, Some("model timed out"))
    );
    assert!(failed.prediction.is_none());
    assert_eq!(report.summary(), "66.7% (2/3 passed, 1 errored)");
}

#[tokio::test]
async fn too_many_errors_stops_the_run_early() {
    let calls = AtomicUsize::new(0);
    let program = |_inputs: Values| {
        calls.fetch_add(1, Ordering::SeqCst);
        async { Err::<Values, ProgramError>("bad key".into()) }
    };
    let err = Evaluator::new(metrics::exact_match("answer", true))
        .concurrency(1)
        .max_errors(Some(1))
        .run_with(program, &devset(10))
        .await
        .unwrap_err();
    assert_eq!(
        err,
        EvaluateError::TooManyErrors {
            count: 2,
            max: 1,
            first: "bad key".to_owned()
        }
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn errors_within_the_limit_are_tolerated() {
    let program = |_inputs: Values| async { Err::<Values, ProgramError>("nope".into()) };
    let report = Evaluator::new(metrics::exact_match("answer", true))
        .max_errors(Some(2))
        .run(&program, &devset(2))
        .await
        .unwrap();
    assert_eq!(report.errors().len(), 2);
}

#[tokio::test]
async fn metric_failures_and_non_finite_scores_become_error_rows() {
    let failing = |_: &Example, _: &Values| -> Result<f64, EvaluateError> {
        Err(EvaluateError::Metric("label is not a number".to_owned()))
    };
    struct Failing<F>(F);
    impl<F: Fn(&Example, &Values) -> Result<f64, EvaluateError> + Send + Sync> Metric for Failing<F> {
        fn score(&self, example: &Example, prediction: &Values) -> Result<f64, EvaluateError> {
            (self.0)(example, prediction)
        }
    }

    let report = Evaluator::new(Failing(failing))
        .run(&doubler, &devset(1))
        .await
        .unwrap();
    let row = &report.rows()[0];
    assert_eq!(
        row.error.as_deref(),
        Some("metric failed: label is not a number")
    );
    assert!(
        row.prediction.is_some(),
        "the prediction is kept for inspection"
    );

    let nan = |_: &Example, _: &Values| f64::NAN;
    let report = Evaluator::new(nan).run(&doubler, &devset(2)).await.unwrap();
    assert_eq!(report.score(), 0.0);
    assert!(report.rows()[0]
        .error
        .as_deref()
        .unwrap()
        .contains("not a finite score"));
}

#[tokio::test]
async fn reports_progress_once_per_example() {
    let ticks = Arc::new(Mutex::new(Vec::new()));
    let recorder = Arc::clone(&ticks);
    Evaluator::new(metrics::exact_match("answer", true))
        .progress(move |done, total| recorder.lock().unwrap().push((done, total)))
        .run(&doubler, &devset(3))
        .await
        .unwrap();
    assert_eq!(*ticks.lock().unwrap(), [(1, 3), (2, 3), (3, 3)]);
}

#[tokio::test]
async fn concurrency_is_bounded() {
    let running = AtomicUsize::new(0);
    let peak = AtomicUsize::new(0);
    let program = |_inputs: Values| {
        let (running, peak) = (&running, &peak);
        async move {
            let now = running.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(5)).await;
            running.fetch_sub(1, Ordering::SeqCst);
            Ok::<_, ProgramError>(Values::new())
        }
    };
    Evaluator::new(metrics::exact_match("answer", true))
        .concurrency(3)
        .run_with(program, &devset(12))
        .await
        .unwrap();
    assert_eq!(peak.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn configuration_and_empty_devsets() {
    let err = Evaluator::new(metrics::f1("answer"))
        .concurrency(0)
        .run(&doubler, &devset(1))
        .await
        .unwrap_err();
    assert_eq!(err.to_string(), "concurrency must be at least 1");

    let report = Evaluator::new(metrics::f1("answer"))
        .run(&doubler, &[])
        .await
        .unwrap();
    assert!(report.is_empty());
    assert_eq!(report.score(), 0.0);
}

#[test]
fn score_predictions_requires_aligned_inputs() {
    let err = cosmos_llm_evaluate::score_predictions(
        &metrics::f1("answer"),
        &devset(2),
        &[Values::new()],
    )
    .unwrap_err();
    assert_eq!(
        err.to_string(),
        "devset has 2 examples but there are 1 predictions"
    );
}
