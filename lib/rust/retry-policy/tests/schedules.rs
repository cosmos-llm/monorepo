//! Schedule behavior asserted from outside the crate.
//!
//! Everything here runs on a [`RecordingClock`], so the suite spends no real
//! time no matter how long the schedules it describes.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use retry_policy::{
    Jitter, ProcessRandomness, RecordingClock, RetryError, RetryPolicy, Retryable, StoppedBecause,
};

/// Retryable, with no requested delay.
#[derive(Debug)]
struct Overloaded;

impl Retryable for Overloaded {
    fn is_retryable(&self) -> bool {
        true
    }
}

/// Retryable, and names its own delay.
#[derive(Debug)]
struct Throttled(Duration);

impl Retryable for Throttled {
    fn is_retryable(&self) -> bool {
        true
    }
    fn retry_after(&self) -> Option<Duration> {
        Some(self.0)
    }
}

/// Not worth retrying.
#[derive(Debug)]
struct BadRequest;

impl Retryable for BadRequest {
    fn is_retryable(&self) -> bool {
        false
    }
}

/// Runs a policy against an always-failing operation and returns its schedule.
async fn schedule_of<E: Retryable>(
    policy: &RetryPolicy,
    mut rng: impl retry_policy::Randomness,
    error: impl Fn() -> E,
) -> (Vec<Duration>, RetryError<E>) {
    let mut clock = RecordingClock::new();
    let result: Result<(), _> = policy
        .run_with(&mut clock, &mut rng, || async { Err(error()) })
        .await;
    (clock.sleeps().to_vec(), result.unwrap_err())
}

#[tokio::test]
async fn a_full_schedule_is_geometric_and_capped() {
    let policy = RetryPolicy::new()
        .max_attempts(6)
        .initial_backoff(Duration::from_secs(1))
        .max_backoff(Duration::from_secs(8))
        .jitter(Jitter::None);

    let (sleeps, err) = schedule_of(&policy, ProcessRandomness::seeded(1), || Overloaded).await;

    // 1, 2, 4, 8, then clamped at 8. Five waits for six attempts.
    assert_eq!(
        sleeps,
        vec![
            Duration::from_secs(1),
            Duration::from_secs(2),
            Duration::from_secs(4),
            Duration::from_secs(8),
            Duration::from_secs(8),
        ]
    );
    assert_eq!(err.attempts_made(), 6);
    assert_eq!(err.total_backoff(), Duration::from_secs(23));
}

#[tokio::test]
async fn concurrent_callers_get_distinct_schedules() {
    // The thundering-herd case the plan calls out. Twenty callers each build
    // their own randomness source, fail identically, and must not retry in
    // lockstep.
    let policy = Arc::new(
        RetryPolicy::new()
            .max_attempts(4)
            .initial_backoff(Duration::from_secs(1))
            .jitter(Jitter::Full),
    );

    let schedules = Arc::new(Mutex::new(Vec::new()));
    let mut handles = Vec::new();

    for _ in 0..20 {
        let policy = policy.clone();
        let schedules = schedules.clone();
        handles.push(tokio::spawn(async move {
            let mut clock = RecordingClock::new();
            let mut rng = ProcessRandomness::new();
            let _: Result<(), _> = policy
                .run_with(&mut clock, &mut rng, || async { Err(Overloaded) })
                .await;
            schedules.lock().unwrap().push(clock.sleeps().to_vec());
        }));
    }
    for handle in handles {
        handle.await.unwrap();
    }

    let schedules = schedules.lock().unwrap();
    assert_eq!(schedules.len(), 20);

    // Distinct is the property that matters. Identical schedules across
    // callers would mean they all come back at the same instant.
    let mut seen: Vec<&Vec<Duration>> = Vec::new();
    for s in schedules.iter() {
        seen.push(s);
    }
    seen.sort();
    seen.dedup();
    assert!(
        seen.len() >= 19,
        "only {} of 20 schedules were distinct",
        seen.len()
    );

    // And every delay stays inside its un-jittered bound.
    for s in schedules.iter() {
        for (i, delay) in s.iter().enumerate() {
            let bound = policy.backoff_for(i as u32 + 1);
            assert!(*delay <= bound, "{delay:?} exceeded {bound:?}");
        }
    }
}

#[tokio::test]
async fn a_requested_delay_is_honored_then_capped() {
    let policy = RetryPolicy::new()
        .max_attempts(4)
        .initial_backoff(Duration::from_millis(50))
        .max_retry_after(Duration::from_secs(30));

    // Inside the cap: obeyed exactly, and not jittered.
    let (sleeps, _) = schedule_of(&policy, ProcessRandomness::seeded(2), || {
        Throttled(Duration::from_secs(12))
    })
    .await;
    assert!(
        sleeps.iter().all(|d| *d == Duration::from_secs(12)),
        "{sleeps:?}"
    );

    // Beyond the cap: refuse rather than hang for an hour.
    let (sleeps, err) = schedule_of(&policy, ProcessRandomness::seeded(3), || {
        Throttled(Duration::from_secs(3600))
    })
    .await;
    assert!(sleeps.is_empty(), "should not have slept at all");
    assert_eq!(
        err.stopped_because,
        StoppedBecause::DelayTooLong {
            requested: Duration::from_secs(3600),
            cap: Duration::from_secs(30),
        }
    );
    assert_eq!(err.attempts_made(), 1);
}

#[tokio::test]
async fn a_non_retryable_failure_never_sleeps() {
    let policy = RetryPolicy::new().max_attempts(10);
    let (sleeps, err) = schedule_of(&policy, ProcessRandomness::seeded(4), || BadRequest).await;

    assert!(sleeps.is_empty());
    assert_eq!(err.attempts_made(), 1);
    assert_eq!(err.stopped_because, StoppedBecause::NotRetryable);
}

#[tokio::test]
async fn an_operation_that_recovers_stops_retrying() {
    let policy = RetryPolicy::new()
        .max_attempts(5)
        .initial_backoff(Duration::from_secs(1))
        .jitter(Jitter::None);

    let mut clock = RecordingClock::new();
    let mut rng = ProcessRandomness::seeded(5);
    let mut calls = 0;

    let result = policy
        .run_with(&mut clock, &mut rng, || {
            calls += 1;
            async move {
                if calls < 3 {
                    Err(Overloaded)
                } else {
                    Ok(calls)
                }
            }
        })
        .await;

    assert_eq!(result.unwrap(), 3);
    assert_eq!(calls, 3);
    // Two waits for two failures; nothing after the success.
    assert_eq!(
        clock.sleeps(),
        &[Duration::from_secs(1), Duration::from_secs(2)]
    );
}

#[tokio::test]
async fn the_default_policy_cannot_stall_for_long() {
    // A default that could hang a caller for minutes would be the wrong
    // default. Worst case here is 100ms + 200ms.
    let policy = RetryPolicy::new().jitter(Jitter::None);
    let (_, err) = schedule_of(&policy, ProcessRandomness::seeded(6), || Overloaded).await;

    assert_eq!(err.attempts_made(), 3);
    assert!(
        err.total_backoff() <= Duration::from_millis(300),
        "default stalled for {:?}",
        err.total_backoff()
    );
}

#[tokio::test]
async fn a_disabled_policy_makes_exactly_one_attempt() {
    let mut clock = RecordingClock::new();
    let mut rng = ProcessRandomness::seeded(7);
    let mut calls = 0;

    let result: Result<(), _> = RetryPolicy::none()
        .run_with(&mut clock, &mut rng, || {
            calls += 1;
            async { Err(Overloaded) }
        })
        .await;

    assert_eq!(calls, 1);
    assert!(clock.sleeps().is_empty());
    assert_eq!(result.unwrap_err().attempts_made(), 1);
}
