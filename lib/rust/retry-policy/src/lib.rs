//! # retry-policy
//!
//! Retry schedules: exponential backoff, jitter, attempt limits, and a record of
//! what actually happened.
//!
//! This crate decides *when* to try again. It does not decide *whether* a
//! failure is worth retrying — that depends on the error, which only the caller
//! understands. Implement [`Retryable`] for your error type and the two
//! concerns stay separate.
//!
//! ## Quick start
//!
//! ```
//! # #[cfg(feature = "tokio")] {
//! use std::time::Duration;
//! use retry_policy::{RetryPolicy, Retryable};
//!
//! #[derive(Debug)]
//! enum FetchError {
//!     Timeout,
//!     NotFound,
//! }
//!
//! impl Retryable for FetchError {
//!     fn is_retryable(&self) -> bool {
//!         matches!(self, Self::Timeout)
//!     }
//! }
//!
//! # tokio_test::block_on(async {
//! let policy = RetryPolicy::new()
//!     .max_attempts(3)
//!     .initial_backoff(Duration::from_millis(1));
//!
//! let mut attempts = 0;
//! let result = policy
//!     .run(|| {
//!         attempts += 1;
//!         async move {
//!             if attempts < 3 { Err(FetchError::Timeout) } else { Ok("body") }
//!         }
//!     })
//!     .await;
//!
//! assert_eq!(result.unwrap(), "body");
//!
//! // A failure that is not worth retrying costs exactly one attempt.
//! let mut calls = 0;
//! let result: Result<&str, _> = policy
//!     .run(|| {
//!         calls += 1;
//!         async { Err(FetchError::NotFound) }
//!     })
//!     .await;
//! assert_eq!(calls, 1);
//! # })
//! # }
//! ```
//!
//! ## What the defaults do
//!
//! [`RetryPolicy::default`] is three attempts, 100ms initial backoff, doubling,
//! capped at 30 seconds, with full jitter. Modest on purpose: a default that
//! retried ten times turns one slow failure into a long one, and against a
//! metered API every retry costs someone money.
//!
//! Jitter defaults to [`Jitter::Full`] because it is not optional in practice.
//! Callers that fail at the same moment and retry on identical schedules arrive
//! together, re-trigger whatever they hit, and fail together again.
//!
//! ## Requested delays
//!
//! When a failure carries its own delay — HTTP `Retry-After`, most often —
//! implement [`Retryable::retry_after`] and it is used instead of the computed
//! curve, unjittered. The server knows its own limits better than an exponential
//! guess does.
//!
//! It is still capped by [`RetryPolicy::max_retry_after`]. A `Retry-After: 3600`
//! is an instruction to hang for an hour, so exceeding the cap stops the loop
//! with [`StoppedBecause::DelayTooLong`] rather than obeying.
//!
//! ## Attempt records
//!
//! Giving up returns a [`RetryError`] holding every attempt: which one, what
//! failed, how long the wait was, and whether that wait was requested rather
//! than computed. That is what makes "why did this take 40 seconds" answerable
//! after the fact instead of requiring instrumentation up front.
//!
//! ```
//! use std::time::Duration;
//! use retry_policy::{Jitter, RecordingClock, RetryPolicy, Retryable, SequenceRandomness};
//!
//! struct Overloaded;
//! impl Retryable for Overloaded {
//!     fn is_retryable(&self) -> bool { true }
//! }
//!
//! # tokio_test::block_on(async {
//! let mut clock = RecordingClock::new();
//! let mut rng = SequenceRandomness::fixed(0.0);
//!
//! let result: Result<(), _> = RetryPolicy::new()
//!     .max_attempts(3)
//!     .initial_backoff(Duration::from_secs(1))
//!     .jitter(Jitter::None)
//!     .run_with(&mut clock, &mut rng, || async { Err(Overloaded) })
//!     .await;
//!
//! let err = result.unwrap_err();
//! assert_eq!(err.attempts_made(), 3);
//! assert_eq!(err.total_backoff(), Duration::from_secs(3));  // 1s + 2s
//! # })
//! ```
//!
//! ## Testing
//!
//! Time and randomness are both injected. [`RecordingClock`] records requested
//! sleeps and returns immediately, so a test asserts a whole schedule in no
//! elapsed time; [`SequenceRandomness`] makes jitter deterministic. This crate's
//! own tests never touch a real clock.
//!
//! [`RetryPolicy::run`] uses a real clock and a built-in jitter source, and is
//! behind the default-on `tokio` feature. Without it the crate still computes
//! and drives schedules through [`RetryPolicy::run_with`], with no dependencies
//! at all.

pub mod attempt;
pub mod clock;
pub mod jitter;
pub mod policy;

pub use attempt::{Attempt, RetryError, StoppedBecause};
pub use clock::{Clock, ProcessRandomness, Randomness, RecordingClock, SequenceRandomness};
pub use jitter::Jitter;
pub use policy::{Decision, RetryPolicy, Retryable};

#[cfg(feature = "tokio")]
pub use clock::TokioClock;
