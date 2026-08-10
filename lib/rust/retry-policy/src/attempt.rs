//! Records of what a retry loop actually did.

use std::time::Duration;

/// Why a retry loop stopped retrying.
///
/// A caller that only sees the final error cannot tell "we gave up" from "it
/// was never worth trying again", and those call for different responses.
///
/// # Examples
///
/// ```
/// use retry_policy::StoppedBecause;
///
/// // Only one of these is worth raising the attempt limit for.
/// assert!(StoppedBecause::AttemptsExhausted.is_exhaustion());
/// assert!(!StoppedBecause::NotRetryable.is_exhaustion());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum StoppedBecause {
    /// Every allowed attempt was used and all of them failed.
    AttemptsExhausted,
    /// The error was classified as not worth retrying, so no further attempt
    /// was made.
    NotRetryable,
    /// The delay the policy would have waited exceeded its cap.
    ///
    /// Distinct from [`Self::AttemptsExhausted`]: attempts remained, but
    /// honoring the requested delay would have stalled longer than the caller
    /// allowed. See
    /// [`RetryPolicy::max_retry_after`](crate::RetryPolicy::max_retry_after).
    DelayTooLong {
        /// The delay that was asked for.
        requested: Duration,
        /// The configured ceiling it exceeded.
        cap: Duration,
    },
}

impl StoppedBecause {
    /// Returns `true` if the loop ran out of attempts.
    ///
    /// # Examples
    ///
    /// ```
    /// use retry_policy::StoppedBecause;
    ///
    /// assert!(StoppedBecause::AttemptsExhausted.is_exhaustion());
    /// ```
    pub fn is_exhaustion(&self) -> bool {
        matches!(self, Self::AttemptsExhausted)
    }
}

/// One failed attempt and the delay that followed it.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use retry_policy::Attempt;
///
/// let attempt = Attempt {
///     number: 1,
///     error: "503 overloaded".to_string(),
///     backoff: Some(Duration::from_millis(250)),
///     honored_retry_after: false,
/// };
/// assert_eq!(attempt.number, 1);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt<E> {
    /// Which attempt this was, counting from 1.
    pub number: u32,
    /// The error it failed with.
    pub error: E,
    /// How long the loop waited afterwards.
    ///
    /// `None` on the final attempt, since nothing followed it.
    pub backoff: Option<Duration>,
    /// Whether `backoff` came from the error's own requested delay rather than
    /// the computed curve.
    ///
    /// Worth recording separately: a schedule that looks nothing like the
    /// configured backoff is explained by this being `true`.
    pub honored_retry_after: bool,
}

/// Every attempt a retry loop made before giving up.
///
/// Returned instead of a bare error so a caller can answer "why did this take
/// so long" without having instrumented the loop in advance.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use retry_policy::{Attempt, RetryError, StoppedBecause};
///
/// let err = RetryError {
///     attempts: vec![
///         Attempt {
///             number: 1,
///             error: "503",
///             backoff: Some(Duration::from_secs(1)),
///             honored_retry_after: false,
///         },
///         Attempt {
///             number: 2,
///             error: "503",
///             backoff: None,
///             honored_retry_after: false,
///         },
///     ],
///     stopped_because: StoppedBecause::AttemptsExhausted,
/// };
///
/// assert_eq!(err.attempts_made(), 2);
/// assert_eq!(err.total_backoff(), Duration::from_secs(1));
/// assert_eq!(*err.last_error(), "503");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryError<E> {
    /// Every attempt, in order. Never empty: the loop always tries once.
    pub attempts: Vec<Attempt<E>>,
    /// Why the loop stopped.
    pub stopped_because: StoppedBecause,
}

impl<E> RetryError<E> {
    /// Returns how many attempts were made.
    ///
    /// # Examples
    ///
    /// ```
    /// use retry_policy::{Attempt, RetryError, StoppedBecause};
    ///
    /// let err = RetryError {
    ///     attempts: vec![Attempt {
    ///         number: 1,
    ///         error: "nope",
    ///         backoff: None,
    ///         honored_retry_after: false,
    ///     }],
    ///     stopped_because: StoppedBecause::NotRetryable,
    /// };
    /// assert_eq!(err.attempts_made(), 1);
    /// ```
    pub fn attempts_made(&self) -> u32 {
        self.attempts.len() as u32
    }

    /// Returns the error from the last attempt.
    ///
    /// # Panics
    ///
    /// Never in practice: a loop always makes at least one attempt, so
    /// `attempts` is never empty. The panic message says so, because a
    /// hand-constructed empty `RetryError` is a bug in the caller.
    ///
    /// # Examples
    ///
    /// ```
    /// use retry_policy::{Attempt, RetryError, StoppedBecause};
    ///
    /// let err = RetryError {
    ///     attempts: vec![Attempt {
    ///         number: 1,
    ///         error: "final",
    ///         backoff: None,
    ///         honored_retry_after: false,
    ///     }],
    ///     stopped_because: StoppedBecause::NotRetryable,
    /// };
    /// assert_eq!(*err.last_error(), "final");
    /// ```
    pub fn last_error(&self) -> &E {
        &self
            .attempts
            .last()
            .expect("a RetryError always holds at least one attempt")
            .error
    }

    /// Consumes this error and returns the last attempt's error.
    ///
    /// For a caller that wants to propagate the underlying failure and discard
    /// the history.
    ///
    /// # Panics
    ///
    /// Never in practice; see [`RetryError::last_error`].
    ///
    /// # Examples
    ///
    /// ```
    /// use retry_policy::{Attempt, RetryError, StoppedBecause};
    ///
    /// let err = RetryError {
    ///     attempts: vec![Attempt {
    ///         number: 1,
    ///         error: String::from("boom"),
    ///         backoff: None,
    ///         honored_retry_after: false,
    ///     }],
    ///     stopped_because: StoppedBecause::NotRetryable,
    /// };
    /// assert_eq!(err.into_last_error(), "boom");
    /// ```
    pub fn into_last_error(self) -> E {
        self.attempts
            .into_iter()
            .next_back()
            .expect("a RetryError always holds at least one attempt")
            .error
    }

    /// Returns the total time spent waiting between attempts.
    ///
    /// Excludes time spent inside the operation itself, which this crate does
    /// not measure.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use retry_policy::{Attempt, RetryError, StoppedBecause};
    ///
    /// let err = RetryError {
    ///     attempts: vec![Attempt {
    ///         number: 1,
    ///         error: "e",
    ///         backoff: Some(Duration::from_secs(2)),
    ///         honored_retry_after: false,
    ///     }],
    ///     stopped_because: StoppedBecause::AttemptsExhausted,
    /// };
    /// assert_eq!(err.total_backoff(), Duration::from_secs(2));
    /// ```
    pub fn total_backoff(&self) -> Duration {
        self.attempts.iter().filter_map(|a| a.backoff).sum()
    }
}

impl<E: std::fmt::Display> std::fmt::Display for RetryError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let attempts = self.attempts_made();
        let waited = self.total_backoff();

        match self.stopped_because {
            StoppedBecause::NotRetryable => {
                write!(f, "failed without retrying: {}", self.last_error())
            }
            StoppedBecause::AttemptsExhausted => write!(
                f,
                "failed after {attempts} attempt(s) over {waited:?} of backoff: {}",
                self.last_error()
            ),
            StoppedBecause::DelayTooLong { requested, cap } => write!(
                f,
                "failed after {attempts} attempt(s): a delay of {requested:?} was \
                 requested, exceeding the {cap:?} cap: {}",
                self.last_error()
            ),
        }
    }
}

impl<E> std::error::Error for RetryError<E> where E: std::fmt::Debug + std::fmt::Display {}

#[cfg(test)]
mod tests {
    use super::*;

    fn attempt(number: u32, backoff: Option<Duration>) -> Attempt<&'static str> {
        Attempt {
            number,
            error: "503",
            backoff,
            honored_retry_after: false,
        }
    }

    fn exhausted() -> RetryError<&'static str> {
        RetryError {
            attempts: vec![
                attempt(1, Some(Duration::from_secs(1))),
                attempt(2, Some(Duration::from_secs(2))),
                attempt(3, None),
            ],
            stopped_because: StoppedBecause::AttemptsExhausted,
        }
    }

    #[test]
    fn totals_are_derived_from_the_attempt_list() {
        let err = exhausted();
        assert_eq!(err.attempts_made(), 3);
        // The final attempt has no backoff after it, so it contributes nothing.
        assert_eq!(err.total_backoff(), Duration::from_secs(3));
        assert_eq!(*err.last_error(), "503");
    }

    #[test]
    fn into_last_error_takes_the_final_failure() {
        let err = RetryError {
            attempts: vec![
                Attempt {
                    number: 1,
                    error: String::from("first"),
                    backoff: Some(Duration::from_secs(1)),
                    honored_retry_after: false,
                },
                Attempt {
                    number: 2,
                    error: String::from("last"),
                    backoff: None,
                    honored_retry_after: false,
                },
            ],
            stopped_because: StoppedBecause::AttemptsExhausted,
        };
        assert_eq!(err.into_last_error(), "last");
    }

    #[test]
    fn display_distinguishes_the_stop_reasons() {
        // A caller reading a log needs to tell these apart without matching.
        let exhausted = exhausted().to_string();
        assert!(exhausted.contains("3 attempt(s)"), "{exhausted}");

        let not_retryable = RetryError {
            attempts: vec![attempt(1, None)],
            stopped_because: StoppedBecause::NotRetryable,
        }
        .to_string();
        assert!(
            not_retryable.contains("without retrying"),
            "{not_retryable}"
        );

        let too_long = RetryError {
            attempts: vec![attempt(1, None)],
            stopped_because: StoppedBecause::DelayTooLong {
                requested: Duration::from_secs(3600),
                cap: Duration::from_secs(60),
            },
        }
        .to_string();
        assert!(too_long.contains("3600s"), "{too_long}");
        assert!(too_long.contains("cap"), "{too_long}");
    }

    #[test]
    fn is_exhaustion_only_for_exhaustion() {
        assert!(StoppedBecause::AttemptsExhausted.is_exhaustion());
        assert!(!StoppedBecause::NotRetryable.is_exhaustion());
        assert!(!StoppedBecause::DelayTooLong {
            requested: Duration::from_secs(1),
            cap: Duration::ZERO,
        }
        .is_exhaustion());
    }

    #[test]
    fn implements_std_error() {
        fn assert_error<E: std::error::Error>(_: &E) {}
        assert_error(&exhausted());
    }
}
