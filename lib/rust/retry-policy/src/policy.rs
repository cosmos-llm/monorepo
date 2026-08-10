//! The retry schedule itself.

use std::time::Duration;

use crate::attempt::{Attempt, RetryError, StoppedBecause};
use crate::clock::{Clock, Randomness};
use crate::jitter::Jitter;

/// What a retry loop should do about a particular failure.
///
/// Implemented by the caller for its own error type. This is the seam that
/// keeps the crate generic: the caller owns classification, this crate owns the
/// schedule.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use retry_policy::Retryable;
///
/// enum ApiError {
///     RateLimited { retry_after: Option<Duration> },
///     BadRequest,
/// }
///
/// impl Retryable for ApiError {
///     fn is_retryable(&self) -> bool {
///         matches!(self, Self::RateLimited { .. })
///     }
///
///     fn retry_after(&self) -> Option<Duration> {
///         match self {
///             Self::RateLimited { retry_after } => *retry_after,
///             _ => None,
///         }
///     }
/// }
///
/// assert!(ApiError::RateLimited { retry_after: None }.is_retryable());
/// assert!(!ApiError::BadRequest.is_retryable());
/// ```
pub trait Retryable {
    /// Returns `true` if retrying could plausibly succeed.
    fn is_retryable(&self) -> bool;

    /// Returns a delay the failing party explicitly asked for, if any.
    ///
    /// For HTTP, this is `Retry-After`. When present it is preferred over the
    /// computed curve: the server knows its own limits better than an
    /// exponential guess does. It is still subject to
    /// [`RetryPolicy::max_retry_after`].
    ///
    /// The default returns `None`, which falls back to computed backoff.
    fn retry_after(&self) -> Option<Duration> {
        None
    }
}

/// An exponential-backoff retry schedule.
///
/// Delays grow as `initial_backoff * multiplier^(attempt - 1)`, clamped to
/// `max_backoff`, then randomized by `jitter`.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use retry_policy::{Jitter, RetryPolicy};
///
/// let policy = RetryPolicy::new()
///     .max_attempts(4)
///     .initial_backoff(Duration::from_millis(200))
///     .jitter(Jitter::None);
///
/// // 200ms, 400ms, 800ms — doubling, with the last attempt not followed by a wait.
/// assert_eq!(policy.backoff_for(1), Duration::from_millis(200));
/// assert_eq!(policy.backoff_for(2), Duration::from_millis(400));
/// assert_eq!(policy.backoff_for(3), Duration::from_millis(800));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct RetryPolicy {
    /// Total attempts, including the first. `1` disables retrying.
    pub max_attempts: u32,
    /// Delay after the first failure, before any multiplier is applied.
    pub initial_backoff: Duration,
    /// Ceiling on a single computed delay.
    pub max_backoff: Duration,
    /// Growth factor applied per attempt.
    pub multiplier: f64,
    /// Randomization applied to each computed delay.
    pub jitter: Jitter,
    /// Ceiling on a delay the failing party requested.
    ///
    /// A server answering `Retry-After: 3600` is telling you to come back in an
    /// hour, and a client that obeys has hung. Exceeding this stops the loop
    /// with [`StoppedBecause::DelayTooLong`] rather than sleeping.
    pub max_retry_after: Duration,
}

impl Default for RetryPolicy {
    /// Three attempts, 100ms initial, doubling, capped at 30s, full jitter.
    ///
    /// Deliberately modest: a default that retried ten times would turn one
    /// slow failure into a long one, and every retry against a paid API costs
    /// someone money.
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(30),
            multiplier: 2.0,
            jitter: Jitter::Full,
            max_retry_after: Duration::from_secs(60),
        }
    }
}

impl RetryPolicy {
    /// Creates a policy with the [`Default`] settings.
    ///
    /// # Examples
    ///
    /// ```
    /// use retry_policy::RetryPolicy;
    ///
    /// assert_eq!(RetryPolicy::new().max_attempts, 3);
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a policy that never retries.
    ///
    /// Useful as an explicit "off" rather than threading an `Option` through a
    /// caller's configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// use retry_policy::RetryPolicy;
    ///
    /// assert_eq!(RetryPolicy::none().max_attempts, 1);
    /// ```
    pub fn none() -> Self {
        Self {
            max_attempts: 1,
            ..Self::default()
        }
    }

    /// Sets the total number of attempts, including the first.
    ///
    /// Zero is treated as one: a policy that makes no attempt at all would
    /// silently skip the caller's work.
    ///
    /// # Examples
    ///
    /// ```
    /// use retry_policy::RetryPolicy;
    ///
    /// assert_eq!(RetryPolicy::new().max_attempts(5).max_attempts, 5);
    /// assert_eq!(RetryPolicy::new().max_attempts(0).max_attempts, 1);
    /// ```
    pub fn max_attempts(mut self, n: u32) -> Self {
        self.max_attempts = n.max(1);
        self
    }

    /// Sets the delay following the first failure.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use retry_policy::RetryPolicy;
    ///
    /// let p = RetryPolicy::new().initial_backoff(Duration::from_secs(1));
    /// assert_eq!(p.initial_backoff, Duration::from_secs(1));
    /// ```
    pub fn initial_backoff(mut self, d: Duration) -> Self {
        self.initial_backoff = d;
        self
    }

    /// Sets the ceiling on a single computed delay.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use retry_policy::RetryPolicy;
    ///
    /// let p = RetryPolicy::new().max_backoff(Duration::from_secs(5));
    /// assert_eq!(p.max_backoff, Duration::from_secs(5));
    /// ```
    pub fn max_backoff(mut self, d: Duration) -> Self {
        self.max_backoff = d;
        self
    }

    /// Sets the growth factor.
    ///
    /// Values below 1.0 are raised to 1.0; a shrinking backoff retries harder
    /// the worse things get, which is the opposite of the point.
    ///
    /// # Examples
    ///
    /// ```
    /// use retry_policy::RetryPolicy;
    ///
    /// assert_eq!(RetryPolicy::new().multiplier(1.5).multiplier, 1.5);
    /// assert_eq!(RetryPolicy::new().multiplier(0.5).multiplier, 1.0);
    /// ```
    pub fn multiplier(mut self, m: f64) -> Self {
        self.multiplier = if m.is_finite() { m.max(1.0) } else { 1.0 };
        self
    }

    /// Sets the jitter strategy.
    ///
    /// # Examples
    ///
    /// ```
    /// use retry_policy::{Jitter, RetryPolicy};
    ///
    /// let p = RetryPolicy::new().jitter(Jitter::None);
    /// assert_eq!(p.jitter, Jitter::None);
    /// ```
    pub fn jitter(mut self, jitter: Jitter) -> Self {
        self.jitter = jitter;
        self
    }

    /// Sets the ceiling on a requested delay.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use retry_policy::RetryPolicy;
    ///
    /// let p = RetryPolicy::new().max_retry_after(Duration::from_secs(10));
    /// assert_eq!(p.max_retry_after, Duration::from_secs(10));
    /// ```
    pub fn max_retry_after(mut self, d: Duration) -> Self {
        self.max_retry_after = d;
        self
    }

    /// Returns the un-jittered delay following attempt `attempt`.
    ///
    /// `attempt` counts from 1. The result is clamped to
    /// [`RetryPolicy::max_backoff`].
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use retry_policy::RetryPolicy;
    ///
    /// let policy = RetryPolicy::new()
    ///     .initial_backoff(Duration::from_secs(1))
    ///     .max_backoff(Duration::from_secs(4));
    ///
    /// assert_eq!(policy.backoff_for(1), Duration::from_secs(1));
    /// assert_eq!(policy.backoff_for(2), Duration::from_secs(2));
    /// assert_eq!(policy.backoff_for(3), Duration::from_secs(4));
    /// // Clamped, not grown to 8.
    /// assert_eq!(policy.backoff_for(4), Duration::from_secs(4));
    /// ```
    pub fn backoff_for(&self, attempt: u32) -> Duration {
        if attempt <= 1 {
            return self.initial_backoff.min(self.max_backoff);
        }

        // Saturate rather than cast: `(u32::MAX - 1) as i32` is *negative*, and
        // a negative exponent shrinks the backoff instead of growing it, so a
        // high attempt number would retry harder than a low one.
        let exponent = i32::try_from(attempt - 1).unwrap_or(i32::MAX);
        let factor = self.multiplier.powi(exponent);

        // The whole comparison happens in floating point, before building a
        // `Duration`. `Duration::mul_f64` panics on a product that overflows its
        // range, and that product is reachable with a finite factor — 1.5^1000
        // is finite but astronomically larger than any representable duration —
        // so checking `factor.is_finite()` alone is not enough.
        let scaled_secs = self.initial_backoff.as_secs_f64() * factor;
        if !scaled_secs.is_finite() || scaled_secs >= self.max_backoff.as_secs_f64() {
            return self.max_backoff;
        }

        Duration::from_secs_f64(scaled_secs).min(self.max_backoff)
    }

    /// Decides what to do after a failed attempt.
    ///
    /// Returns the delay to wait before the next attempt, or why the loop should
    /// stop. Separated from [`RetryPolicy::run`] so the decision is testable
    /// without a clock, and so a caller driving its own loop can reuse it.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use retry_policy::{Decision, Jitter, RetryPolicy, Retryable, SequenceRandomness};
    ///
    /// #[derive(Debug)]
    /// struct Transient;
    /// impl Retryable for Transient {
    ///     fn is_retryable(&self) -> bool { true }
    /// }
    ///
    /// let policy = RetryPolicy::new()
    ///     .max_attempts(2)
    ///     .initial_backoff(Duration::from_secs(1))
    ///     .jitter(Jitter::None);
    /// let mut rng = SequenceRandomness::fixed(0.0);
    ///
    /// // After attempt 1 of 2, wait and try again.
    /// assert!(matches!(
    ///     policy.decide(&Transient, 1, &mut rng),
    ///     Decision::RetryAfter { .. }
    /// ));
    ///
    /// // After attempt 2 of 2, there is nothing left.
    /// assert!(matches!(policy.decide(&Transient, 2, &mut rng), Decision::Stop(_)));
    /// ```
    pub fn decide<E, R>(&self, error: &E, attempt: u32, rng: &mut R) -> Decision
    where
        E: Retryable,
        R: Randomness + ?Sized,
    {
        if !error.is_retryable() {
            return Decision::Stop(StoppedBecause::NotRetryable);
        }

        if attempt >= self.max_attempts {
            return Decision::Stop(StoppedBecause::AttemptsExhausted);
        }

        // A requested delay wins over the computed curve, but not over the cap.
        // Jitter is deliberately not applied: the server named a number, and
        // randomizing it downward means ignoring what it asked for.
        if let Some(requested) = error.retry_after() {
            if requested > self.max_retry_after {
                return Decision::Stop(StoppedBecause::DelayTooLong {
                    requested,
                    cap: self.max_retry_after,
                });
            }
            return Decision::RetryAfter {
                delay: requested,
                honored_retry_after: true,
            };
        }

        Decision::RetryAfter {
            delay: self.jitter.apply(self.backoff_for(attempt), rng.next_f64()),
            honored_retry_after: false,
        }
    }

    /// Runs `operation` until it succeeds or the policy gives up.
    ///
    /// Waits on `clock` between attempts and draws jitter from `rng`, both
    /// injected so this is testable without elapsed time. For a real clock, see
    /// [`RetryPolicy::run`].
    ///
    /// # Errors
    ///
    /// Returns a [`RetryError`] holding every attempt and why the loop stopped.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use retry_policy::{Jitter, RecordingClock, RetryPolicy, Retryable, SequenceRandomness};
    ///
    /// #[derive(Debug)]
    /// struct Transient;
    /// impl Retryable for Transient {
    ///     fn is_retryable(&self) -> bool { true }
    /// }
    ///
    /// # tokio_test::block_on(async {
    /// let policy = RetryPolicy::new()
    ///     .max_attempts(3)
    ///     .initial_backoff(Duration::from_secs(1))
    ///     .jitter(Jitter::None);
    ///
    /// let mut clock = RecordingClock::new();
    /// let mut rng = SequenceRandomness::fixed(0.0);
    /// let mut calls = 0;
    ///
    /// let result = policy
    ///     .run_with(&mut clock, &mut rng, || {
    ///         calls += 1;
    ///         async move {
    ///             if calls < 3 { Err(Transient) } else { Ok(calls) }
    ///         }
    ///     })
    ///     .await;
    ///
    /// assert_eq!(result.unwrap(), 3);
    /// // Two failures, so two waits: 1s then 2s.
    /// assert_eq!(clock.sleeps(), &[Duration::from_secs(1), Duration::from_secs(2)]);
    /// # })
    /// ```
    pub async fn run_with<T, E, F, Fut, C, R>(
        &self,
        clock: &mut C,
        rng: &mut R,
        mut operation: F,
    ) -> Result<T, RetryError<E>>
    where
        E: Retryable,
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
        C: Clock + ?Sized,
        R: Randomness + ?Sized,
    {
        let mut attempts: Vec<Attempt<E>> = Vec::new();

        for number in 1..=self.max_attempts {
            match operation().await {
                Ok(value) => return Ok(value),
                Err(error) => match self.decide(&error, number, rng) {
                    Decision::RetryAfter {
                        delay,
                        honored_retry_after,
                    } => {
                        attempts.push(Attempt {
                            number,
                            error,
                            backoff: Some(delay),
                            honored_retry_after,
                        });
                        clock.sleep(delay).await;
                    }
                    Decision::Stop(stopped_because) => {
                        attempts.push(Attempt {
                            number,
                            error,
                            backoff: None,
                            honored_retry_after: false,
                        });
                        return Err(RetryError {
                            attempts,
                            stopped_because,
                        });
                    }
                },
            }
        }

        // Reached only if `max_attempts` failures all decided to retry, which
        // `decide` prevents by stopping on the final attempt. Kept as a
        // correctness backstop rather than an `unreachable!`.
        Err(RetryError {
            attempts,
            stopped_because: StoppedBecause::AttemptsExhausted,
        })
    }
}

/// What [`RetryPolicy::decide`] concluded about a failed attempt.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use retry_policy::Decision;
///
/// let decision = Decision::RetryAfter {
///     delay: Duration::from_secs(1),
///     honored_retry_after: false,
/// };
/// assert!(decision.is_retry());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Decision {
    /// Wait `delay`, then try again.
    RetryAfter {
        /// How long to wait.
        delay: Duration,
        /// Whether `delay` came from the error rather than the computed curve.
        honored_retry_after: bool,
    },
    /// Do not try again.
    Stop(StoppedBecause),
}

impl Decision {
    /// Returns `true` if this decision calls for another attempt.
    ///
    /// # Examples
    ///
    /// ```
    /// use retry_policy::{Decision, StoppedBecause};
    ///
    /// assert!(!Decision::Stop(StoppedBecause::NotRetryable).is_retry());
    /// ```
    pub fn is_retry(&self) -> bool {
        matches!(self, Self::RetryAfter { .. })
    }

    /// Returns the delay, if this decision calls for another attempt.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use retry_policy::Decision;
    ///
    /// let d = Decision::RetryAfter {
    ///     delay: Duration::from_secs(2),
    ///     honored_retry_after: true,
    /// };
    /// assert_eq!(d.delay(), Some(Duration::from_secs(2)));
    /// ```
    pub fn delay(&self) -> Option<Duration> {
        match self {
            Self::RetryAfter { delay, .. } => Some(*delay),
            Self::Stop(_) => None,
        }
    }
}

#[cfg(feature = "tokio")]
impl RetryPolicy {
    /// Runs `operation` on a real clock, drawing jitter from a built-in source.
    ///
    /// The convenience form of [`RetryPolicy::run_with`]. The randomness is a
    /// small deterministic-per-process hash rather than a cryptographic RNG;
    /// jitter only needs to decorrelate callers, and pulling in a `rand`
    /// dependency for it is not worth the cost. A caller who wants a specific
    /// RNG should use [`RetryPolicy::run_with`].
    ///
    /// # Errors
    ///
    /// Returns a [`RetryError`] holding every attempt and why the loop stopped.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use retry_policy::{RetryPolicy, Retryable};
    ///
    /// #[derive(Debug)]
    /// struct Transient;
    /// impl Retryable for Transient {
    ///     fn is_retryable(&self) -> bool { true }
    /// }
    ///
    /// # tokio_test::block_on(async {
    /// let policy = RetryPolicy::new()
    ///     .max_attempts(2)
    ///     .initial_backoff(Duration::from_millis(1));
    ///
    /// let mut calls = 0;
    /// let result: Result<u32, _> = policy
    ///     .run(|| {
    ///         calls += 1;
    ///         async move {
    ///             if calls < 2 { Err(Transient) } else { Ok(calls) }
    ///         }
    ///     })
    ///     .await;
    ///
    /// assert_eq!(result.unwrap(), 2);
    /// # })
    /// ```
    pub async fn run<T, E, F, Fut>(&self, operation: F) -> Result<T, RetryError<E>>
    where
        E: Retryable,
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
    {
        let mut clock = crate::clock::TokioClock;
        let mut rng = crate::clock::ProcessRandomness::new();
        self.run_with(&mut clock, &mut rng, operation).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{RecordingClock, SequenceRandomness};

    /// Retryable, with no requested delay.
    #[derive(Debug)]
    struct Transient;
    impl Retryable for Transient {
        fn is_retryable(&self) -> bool {
            true
        }
    }

    /// Never worth retrying.
    #[derive(Debug)]
    struct Permanent;
    impl Retryable for Permanent {
        fn is_retryable(&self) -> bool {
            false
        }
    }

    /// Retryable, and asks for a specific delay.
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

    fn deterministic() -> RetryPolicy {
        RetryPolicy::new()
            .initial_backoff(Duration::from_secs(1))
            .max_backoff(Duration::from_secs(60))
            .jitter(Jitter::None)
    }

    #[test]
    fn backoff_grows_geometrically_then_clamps() {
        let policy = deterministic().max_backoff(Duration::from_secs(4));
        assert_eq!(policy.backoff_for(1), Duration::from_secs(1));
        assert_eq!(policy.backoff_for(2), Duration::from_secs(2));
        assert_eq!(policy.backoff_for(3), Duration::from_secs(4));
        assert_eq!(policy.backoff_for(4), Duration::from_secs(4));
        assert_eq!(policy.backoff_for(50), Duration::from_secs(4));
    }

    #[test]
    fn backoff_for_attempt_zero_is_the_initial_delay() {
        // Guards against an underflow in the exponent.
        assert_eq!(deterministic().backoff_for(0), Duration::from_secs(1));
    }

    #[test]
    fn an_enormous_exponent_does_not_panic() {
        // `multiplier.powi` overflows to infinity here; `mul_f64` would panic
        // on that, so it has to be caught before the multiply.
        let policy = deterministic().multiplier(10.0);
        assert_eq!(policy.backoff_for(u32::MAX), policy.max_backoff);
        assert_eq!(policy.backoff_for(400), policy.max_backoff);
    }

    #[test]
    fn backoff_never_decreases_as_attempts_rise() {
        // The invariant that a naive `as i32` cast breaks: past i32::MAX the
        // exponent goes negative and the curve inverts, so a late attempt
        // retries harder than an early one.
        for multiplier in [1.0, 1.5, 2.0, 10.0] {
            let policy = deterministic().multiplier(multiplier);
            for attempt in [1u32, 2, 3, 31, 32, 33, 1_000, u32::MAX - 1, u32::MAX] {
                let previous = policy.backoff_for(attempt.saturating_sub(1));
                let current = policy.backoff_for(attempt);
                assert!(
                    current >= previous,
                    "multiplier {multiplier}, attempt {attempt}: {current:?} < {previous:?}"
                );
            }
        }
    }

    #[test]
    fn initial_backoff_above_the_cap_is_clamped() {
        let policy = RetryPolicy::new()
            .initial_backoff(Duration::from_secs(90))
            .max_backoff(Duration::from_secs(30));
        assert_eq!(policy.backoff_for(1), Duration::from_secs(30));
    }

    #[test]
    fn degenerate_builder_values_are_corrected() {
        assert_eq!(RetryPolicy::new().max_attempts(0).max_attempts, 1);
        assert_eq!(RetryPolicy::new().multiplier(0.0).multiplier, 1.0);
        assert_eq!(RetryPolicy::new().multiplier(-3.0).multiplier, 1.0);
        assert_eq!(RetryPolicy::new().multiplier(f64::NAN).multiplier, 1.0);
    }

    #[test]
    fn a_non_retryable_error_stops_immediately() {
        let mut rng = SequenceRandomness::fixed(0.0);
        assert_eq!(
            deterministic().decide(&Permanent, 1, &mut rng),
            Decision::Stop(StoppedBecause::NotRetryable)
        );
    }

    #[test]
    fn the_final_attempt_stops_as_exhausted() {
        let policy = deterministic().max_attempts(3);
        let mut rng = SequenceRandomness::fixed(0.0);
        assert!(policy.decide(&Transient, 2, &mut rng).is_retry());
        assert_eq!(
            policy.decide(&Transient, 3, &mut rng),
            Decision::Stop(StoppedBecause::AttemptsExhausted)
        );
    }

    #[test]
    fn a_requested_delay_overrides_the_computed_curve() {
        // The server named 7s; the curve would have said 1s.
        let policy = deterministic().max_attempts(5);
        let mut rng = SequenceRandomness::fixed(0.0);
        let decision = policy.decide(&Throttled(Duration::from_secs(7)), 1, &mut rng);
        assert_eq!(
            decision,
            Decision::RetryAfter {
                delay: Duration::from_secs(7),
                honored_retry_after: true,
            }
        );
    }

    #[test]
    fn a_requested_delay_is_not_jittered() {
        // Randomizing it downward means ignoring what the server asked for.
        let policy = deterministic().max_attempts(5).jitter(Jitter::Full);
        let mut rng = SequenceRandomness::fixed(0.0);
        assert_eq!(
            policy
                .decide(&Throttled(Duration::from_secs(7)), 1, &mut rng)
                .delay(),
            Some(Duration::from_secs(7))
        );
    }

    #[test]
    fn a_requested_delay_beyond_the_cap_stops_the_loop() {
        // Retry-After: 3600 should fail rather than sleep for an hour.
        let policy = deterministic()
            .max_attempts(5)
            .max_retry_after(Duration::from_secs(60));
        let mut rng = SequenceRandomness::fixed(0.0);

        assert_eq!(
            policy.decide(&Throttled(Duration::from_secs(3600)), 1, &mut rng),
            Decision::Stop(StoppedBecause::DelayTooLong {
                requested: Duration::from_secs(3600),
                cap: Duration::from_secs(60),
            })
        );

        // Exactly at the cap is still allowed.
        assert!(policy
            .decide(&Throttled(Duration::from_secs(60)), 1, &mut rng)
            .is_retry());
    }

    #[tokio::test]
    async fn a_successful_first_attempt_never_sleeps() {
        let mut clock = RecordingClock::new();
        let mut rng = SequenceRandomness::fixed(0.0);

        let result: Result<u32, RetryError<Transient>> = deterministic()
            .run_with(&mut clock, &mut rng, || async { Ok(1) })
            .await;

        assert_eq!(result.unwrap(), 1);
        assert!(clock.sleeps().is_empty());
    }

    #[tokio::test]
    async fn a_non_retryable_failure_costs_one_attempt_and_no_delay() {
        let mut clock = RecordingClock::new();
        let mut rng = SequenceRandomness::fixed(0.0);
        let mut calls = 0;

        let result: Result<u32, _> = deterministic()
            .max_attempts(5)
            .run_with(&mut clock, &mut rng, || {
                calls += 1;
                async { Err(Permanent) }
            })
            .await;

        let err = result.unwrap_err();
        assert_eq!(calls, 1);
        assert_eq!(err.attempts_made(), 1);
        assert_eq!(err.stopped_because, StoppedBecause::NotRetryable);
        assert!(clock.sleeps().is_empty());
        assert_eq!(err.total_backoff(), Duration::ZERO);
    }

    #[tokio::test]
    async fn retries_until_success_and_records_the_schedule() {
        let mut clock = RecordingClock::new();
        let mut rng = SequenceRandomness::fixed(0.0);
        let mut calls = 0;

        let result = deterministic()
            .max_attempts(4)
            .run_with(&mut clock, &mut rng, || {
                calls += 1;
                async move {
                    if calls < 3 {
                        Err(Transient)
                    } else {
                        Ok(calls)
                    }
                }
            })
            .await;

        assert_eq!(result.unwrap(), 3);
        assert_eq!(
            clock.sleeps(),
            &[Duration::from_secs(1), Duration::from_secs(2)]
        );
    }

    #[tokio::test]
    async fn exhaustion_reports_every_attempt() {
        let mut clock = RecordingClock::new();
        let mut rng = SequenceRandomness::fixed(0.0);

        let result: Result<u32, _> = deterministic()
            .max_attempts(3)
            .run_with(&mut clock, &mut rng, || async { Err(Transient) })
            .await;

        let err = result.unwrap_err();
        assert_eq!(err.attempts_made(), 3);
        assert_eq!(err.stopped_because, StoppedBecause::AttemptsExhausted);

        // The first two have a backoff, the last does not.
        assert_eq!(err.attempts[0].backoff, Some(Duration::from_secs(1)));
        assert_eq!(err.attempts[1].backoff, Some(Duration::from_secs(2)));
        assert_eq!(err.attempts[2].backoff, None);
        assert_eq!(err.total_backoff(), Duration::from_secs(3));

        // Attempt numbers are 1-based and contiguous.
        let numbers: Vec<u32> = err.attempts.iter().map(|a| a.number).collect();
        assert_eq!(numbers, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn max_attempts_of_one_makes_a_single_attempt() {
        let mut clock = RecordingClock::new();
        let mut rng = SequenceRandomness::fixed(0.0);
        let mut calls = 0;

        let result: Result<u32, _> = RetryPolicy::none()
            .run_with(&mut clock, &mut rng, || {
                calls += 1;
                async { Err(Transient) }
            })
            .await;

        assert_eq!(calls, 1);
        assert_eq!(
            result.unwrap_err().stopped_because,
            StoppedBecause::AttemptsExhausted
        );
        assert!(clock.sleeps().is_empty());
    }

    #[tokio::test]
    async fn honored_retry_after_is_recorded_per_attempt() {
        let mut clock = RecordingClock::new();
        let mut rng = SequenceRandomness::fixed(0.0);

        let result: Result<u32, _> = deterministic()
            .max_attempts(2)
            .run_with(&mut clock, &mut rng, || async {
                Err(Throttled(Duration::from_secs(5)))
            })
            .await;

        let err = result.unwrap_err();
        // The schedule looks nothing like the configured curve; this flag is
        // what explains why.
        assert!(err.attempts[0].honored_retry_after);
        assert_eq!(clock.sleeps(), &[Duration::from_secs(5)]);
    }

    #[tokio::test]
    async fn full_jitter_produces_distinct_schedules_for_distinct_draws() {
        // Two callers failing together must not retry in lockstep.
        async fn schedule(draws: Vec<f64>) -> Vec<Duration> {
            let mut clock = RecordingClock::new();
            let mut rng = SequenceRandomness::new(draws);
            let _: Result<u32, _> = RetryPolicy::new()
                .max_attempts(4)
                .initial_backoff(Duration::from_secs(1))
                .jitter(Jitter::Full)
                .run_with(&mut clock, &mut rng, || async { Err(Transient) })
                .await;
            clock.sleeps().to_vec()
        }

        let first = schedule(vec![0.1, 0.2, 0.3]).await;
        let second = schedule(vec![0.9, 0.8, 0.7]).await;

        assert_ne!(first, second);
        // And each is bounded by its un-jittered curve.
        let policy = deterministic();
        for (i, delay) in first.iter().enumerate() {
            assert!(*delay <= policy.backoff_for(i as u32 + 1));
        }
    }

    #[tokio::test]
    async fn jitter_none_reproduces_the_exact_curve() {
        let mut clock = RecordingClock::new();
        let mut rng = SequenceRandomness::fixed(0.7);

        let _: Result<u32, _> = deterministic()
            .max_attempts(4)
            .run_with(&mut clock, &mut rng, || async { Err(Transient) })
            .await;

        assert_eq!(
            clock.sleeps(),
            &[
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(4)
            ]
        );
    }
}
