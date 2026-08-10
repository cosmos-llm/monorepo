//! Randomization applied to a computed backoff delay.

use std::time::Duration;

/// How much randomness to apply to a computed backoff delay.
///
/// Jitter is not decoration. Several callers that fail at the same moment and
/// retry on identical schedules arrive together, re-trigger whatever they hit,
/// and fail together again. Spreading the retries is what breaks that cycle.
///
/// [`Jitter::Full`] is the default because it spreads the widest. The others
/// exist for cases where a caller needs a floor on how long it waits.
///
/// # Examples
///
/// ```
/// use retry_policy::Jitter;
///
/// assert_eq!(Jitter::default(), Jitter::Full);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Jitter {
    /// Sleep a uniform random duration in `[0, backoff]`.
    ///
    /// The widest spread, and the reason it is the default. The tradeoff is
    /// that an individual retry may fire almost immediately.
    #[default]
    Full,
    /// Sleep a uniform random duration in `[backoff / 2, backoff]`.
    ///
    /// Keeps a floor under the delay while still spreading callers apart. Use
    /// this when retrying too eagerly is itself a problem.
    Equal,
    /// No randomization: sleep exactly the computed backoff.
    ///
    /// Only sensible for a single caller, or when reproducibility matters more
    /// than avoiding collisions. Concurrent callers will synchronize.
    None,
}

impl Jitter {
    /// Applies this jitter to `backoff`, given a random value in `[0, 1)`.
    ///
    /// Taking `random` as a parameter rather than drawing it internally is what
    /// makes schedules testable: a test supplies a known sequence and asserts
    /// exact delays, with no dependency on a global RNG.
    ///
    /// Values outside `[0, 1)` are clamped, and non-finite values are treated
    /// as zero, so a misbehaving source cannot produce a negative or absurd
    /// delay.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use retry_policy::Jitter;
    ///
    /// let backoff = Duration::from_secs(4);
    ///
    /// // Full jitter scales the whole range.
    /// assert_eq!(Jitter::Full.apply(backoff, 0.25), Duration::from_secs(1));
    ///
    /// // Equal jitter keeps half as a floor.
    /// assert_eq!(Jitter::Equal.apply(backoff, 0.0), Duration::from_secs(2));
    ///
    /// // None ignores the random value entirely.
    /// assert_eq!(Jitter::None.apply(backoff, 0.9), backoff);
    /// ```
    pub fn apply(&self, backoff: Duration, random: f64) -> Duration {
        let r = if random.is_finite() {
            random.clamp(0.0, 1.0)
        } else {
            0.0
        };

        match self {
            Self::None => backoff,
            Self::Full => backoff.mul_f64(r),
            // Half fixed, half randomized.
            Self::Equal => backoff.mul_f64(0.5 + r * 0.5),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FOUR: Duration = Duration::from_secs(4);

    #[test]
    fn full_jitter_spans_zero_to_backoff() {
        assert_eq!(Jitter::Full.apply(FOUR, 0.0), Duration::ZERO);
        assert_eq!(Jitter::Full.apply(FOUR, 0.5), Duration::from_secs(2));
        assert_eq!(Jitter::Full.apply(FOUR, 1.0), FOUR);
    }

    #[test]
    fn equal_jitter_never_goes_below_half() {
        // The point of Equal: a floor under the delay.
        assert_eq!(Jitter::Equal.apply(FOUR, 0.0), Duration::from_secs(2));
        assert_eq!(Jitter::Equal.apply(FOUR, 1.0), FOUR);
        for r in [0.0, 0.1, 0.33, 0.5, 0.99, 1.0] {
            let d = Jitter::Equal.apply(FOUR, r);
            assert!(d >= FOUR / 2, "{r} gave {d:?}");
            assert!(d <= FOUR, "{r} gave {d:?}");
        }
    }

    #[test]
    fn none_ignores_the_random_value() {
        for r in [0.0, 0.5, 1.0] {
            assert_eq!(Jitter::None.apply(FOUR, r), FOUR);
        }
    }

    #[test]
    fn out_of_range_randomness_is_clamped() {
        // A misbehaving source must not produce a negative or oversized delay.
        assert_eq!(Jitter::Full.apply(FOUR, -1.0), Duration::ZERO);
        assert_eq!(Jitter::Full.apply(FOUR, 2.0), FOUR);
        assert_eq!(Jitter::Full.apply(FOUR, f64::NAN), Duration::ZERO);
        assert_eq!(Jitter::Full.apply(FOUR, f64::INFINITY), Duration::ZERO);
    }

    #[test]
    fn zero_backoff_stays_zero() {
        for jitter in [Jitter::Full, Jitter::Equal, Jitter::None] {
            assert_eq!(jitter.apply(Duration::ZERO, 0.7), Duration::ZERO);
        }
    }

    #[test]
    fn full_is_the_default() {
        // Documented behavior: the widest spread unless a caller opts out.
        assert_eq!(Jitter::default(), Jitter::Full);
    }
}
