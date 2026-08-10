# retry-policy

Retry schedules for Rust: exponential backoff, jitter, attempt limits, and a
record of what actually happened.

This crate decides **when** to try again. It does not decide **whether** a
failure is worth retrying — that depends on the error, and only you know your
errors. Implement `Retryable` and the two concerns stay apart.

```rust
use std::time::Duration;
use retry_policy::{RetryPolicy, Retryable};

#[derive(Debug)]
enum FetchError {
    Timeout,
    NotFound,
}

impl Retryable for FetchError {
    fn is_retryable(&self) -> bool {
        matches!(self, Self::Timeout)
    }
}

let policy = RetryPolicy::new()
    .max_attempts(4)
    .initial_backoff(Duration::from_millis(200));

let body = policy.run(|| async { fetch().await }).await?;
```

A `NotFound` fails on the first attempt with no delay. A `Timeout` is retried on
a 200ms, 400ms, 800ms curve, each delay randomized.

## Why the defaults look like this

**Three attempts, 100ms initial, doubling, capped at 30s.** Modest on purpose. A
default that retried ten times turns one slow failure into a long one, and
against a metered API every retry costs someone money.

**Full jitter, on by default.** Not decoration. Callers that fail at the same
moment and retry on identical schedules arrive together, re-trigger whatever
they hit, and fail together again. `Jitter::Equal` keeps a floor under the delay;
`Jitter::None` exists for single callers and reproducible tests.

## Requested delays

When a failure carries its own delay — HTTP `Retry-After` is the common case —
implement `Retryable::retry_after` and it is used instead of the computed curve,
unjittered. The server knows its own limits better than an exponential guess
does.

It is still capped. `Retry-After: 3600` is an instruction to hang for an hour, so
exceeding `max_retry_after` stops the loop with `StoppedBecause::DelayTooLong`
rather than obeying it.

## Attempt records

Giving up returns a `RetryError` holding every attempt: which one, what failed,
how long the wait was, and whether that wait was requested rather than computed.

```rust
let err = policy.run(|| async { always_fails().await }).await.unwrap_err();

eprintln!("gave up after {} attempts", err.attempts_made());
eprintln!("spent {:?} waiting", err.total_backoff());
```

That makes "why did this take 40 seconds" answerable after the fact, instead of
requiring you to have instrumented the loop in advance.

## Testing

Time and randomness are both injected, so a test asserts a whole schedule in no
elapsed time:

```rust
use retry_policy::{Jitter, RecordingClock, RetryPolicy, SequenceRandomness};

let mut clock = RecordingClock::new();
let mut rng = SequenceRandomness::fixed(0.0);

let policy = RetryPolicy::new()
    .max_attempts(3)
    .initial_backoff(Duration::from_secs(1))
    .jitter(Jitter::None);

let _ = policy.run_with(&mut clock, &mut rng, || async { Err(Timeout) }).await;

// No real seconds were spent.
assert_eq!(clock.sleeps(), &[Duration::from_secs(1), Duration::from_secs(2)]);
```

This crate's own suite never touches a real clock, which is why it runs in
milliseconds while describing schedules that span minutes.

## Features

| Feature | Default | Gives you |
|---|---|---|
| `tokio` | yes | `RetryPolicy::run`, which sleeps on a real clock |

Without it the crate has **no dependencies at all** and still computes schedules
and drives them through `run_with` with your own clock.

## License

MIT
