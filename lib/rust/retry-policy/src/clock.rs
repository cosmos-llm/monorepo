//! Time and randomness sources, injectable so schedules are testable.

use std::time::Duration;

/// Supplies the randomness a [`Jitter`](crate::Jitter) needs.
///
/// A trait rather than a direct `rand` dependency for two reasons: this crate
/// stays dependency-free, and a test can supply a known sequence and assert
/// exact delays. Production callers wire in whatever RNG they already have.
///
/// # Examples
///
/// ```
/// use retry_policy::Randomness;
///
/// /// Always picks the midpoint of the jitter range.
/// struct Midpoint;
///
/// impl Randomness for Midpoint {
///     fn next_f64(&mut self) -> f64 {
///         0.5
///     }
/// }
///
/// assert_eq!(Midpoint.next_f64(), 0.5);
/// ```
pub trait Randomness {
    /// Returns a value in `[0, 1)`.
    ///
    /// Out-of-range and non-finite values are clamped by
    /// [`Jitter::apply`](crate::Jitter::apply), so an imperfect implementation
    /// degrades rather than producing a nonsense delay.
    fn next_f64(&mut self) -> f64;
}

/// A deterministic [`Randomness`] cycling through a fixed sequence.
///
/// This is what makes the schedule tests in this crate exact. It is also useful
/// to a consumer testing its own retry behavior, which is why it is public
/// rather than test-only.
///
/// # Examples
///
/// ```
/// use retry_policy::{Randomness, SequenceRandomness};
///
/// let mut rng = SequenceRandomness::new(vec![0.0, 0.5]);
/// assert_eq!(rng.next_f64(), 0.0);
/// assert_eq!(rng.next_f64(), 0.5);
/// // The sequence repeats rather than running out.
/// assert_eq!(rng.next_f64(), 0.0);
/// ```
#[derive(Debug, Clone)]
pub struct SequenceRandomness {
    values: Vec<f64>,
    index: usize,
}

impl SequenceRandomness {
    /// Creates a source cycling through `values`.
    ///
    /// An empty sequence yields `0.0` forever, so a caller cannot accidentally
    /// panic a retry loop by passing one.
    ///
    /// # Examples
    ///
    /// ```
    /// use retry_policy::{Randomness, SequenceRandomness};
    ///
    /// let mut empty = SequenceRandomness::new(vec![]);
    /// assert_eq!(empty.next_f64(), 0.0);
    /// ```
    pub fn new(values: Vec<f64>) -> Self {
        Self { values, index: 0 }
    }

    /// Creates a source that always returns the same value.
    ///
    /// # Examples
    ///
    /// ```
    /// use retry_policy::{Randomness, SequenceRandomness};
    ///
    /// let mut rng = SequenceRandomness::fixed(0.25);
    /// assert_eq!(rng.next_f64(), 0.25);
    /// assert_eq!(rng.next_f64(), 0.25);
    /// ```
    pub fn fixed(value: f64) -> Self {
        Self::new(vec![value])
    }
}

impl Randomness for SequenceRandomness {
    fn next_f64(&mut self) -> f64 {
        if self.values.is_empty() {
            return 0.0;
        }
        let value = self.values[self.index % self.values.len()];
        self.index = self.index.wrapping_add(1);
        value
    }
}

/// A [`Randomness`] good enough to decorrelate retries, with no dependencies.
///
/// Jitter has one job: stop concurrent callers from retrying in lockstep. That
/// needs values that differ between callers and between draws, not statistical
/// quality, so this uses a small xorshift seeded from the clock and the
/// allocation address of the state itself. Pulling in `rand` to do better would
/// buy nothing a retry loop can use.
///
/// Not suitable for anything security-relevant. Use a real RNG through
/// [`Randomness`] if you need one.
///
/// # Examples
///
/// ```
/// use retry_policy::{ProcessRandomness, Randomness};
///
/// let mut rng = ProcessRandomness::new();
/// for _ in 0..100 {
///     let r = rng.next_f64();
///     assert!((0.0..1.0).contains(&r), "{r} out of range");
/// }
/// ```
#[derive(Debug, Clone)]
pub struct ProcessRandomness {
    state: u64,
}

impl Default for ProcessRandomness {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessRandomness {
    /// Creates a source seeded from the current time and this value's address.
    ///
    /// Two sources created in the same process still differ, which is the
    /// property that matters when several concurrent callers each build one.
    ///
    /// # Examples
    ///
    /// ```
    /// use retry_policy::{ProcessRandomness, Randomness};
    ///
    /// let mut rng = ProcessRandomness::new();
    /// let _ = rng.next_f64();
    /// ```
    pub fn new() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);

        let mut seed = Self { state: 0 };
        // Mixing in the address distinguishes sources built in the same
        // nanosecond, which a concurrent batch will do.
        let addr = &seed as *const _ as u64;
        seed.state = (nanos ^ addr).max(1);
        seed
    }

    /// Creates a source from an explicit seed, for reproducible sequences.
    ///
    /// # Examples
    ///
    /// ```
    /// use retry_policy::{ProcessRandomness, Randomness};
    ///
    /// let a: Vec<f64> = (0..4).map(|_| ProcessRandomness::seeded(7).next_f64()).collect();
    /// // Same seed, same first draw every time.
    /// assert!(a.iter().all(|v| *v == a[0]));
    /// ```
    pub fn seeded(seed: u64) -> Self {
        Self { state: seed.max(1) }
    }
}

impl Randomness for ProcessRandomness {
    fn next_f64(&mut self) -> f64 {
        // xorshift64. A zero state is fixed at construction, so it cannot
        // collapse to always returning zero.
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;

        // 53 bits is the mantissa width, so this divides exactly and cannot
        // round up to 1.0.
        (x >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// Sleeps for a duration.
///
/// Implemented for a real clock by [`TokioClock`] and for tests by
/// [`RecordingClock`], which records what it was asked to sleep and returns
/// immediately. Per the decomposition principle, this crate's own tests use the
/// latter exclusively: a retry test that actually slept would be slow and
/// flaky.
pub trait Clock {
    /// Waits for `duration`.
    fn sleep(&mut self, duration: Duration) -> impl std::future::Future<Output = ()> + Send;
}

/// A [`Clock`] that records requested sleeps without waiting.
///
/// A schedule is a sequence of durations, so asserting on the recorded sleeps
/// tests the whole policy without any elapsed time.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use retry_policy::{Clock, RecordingClock};
///
/// # tokio_test::block_on(async {
/// let mut clock = RecordingClock::new();
/// clock.sleep(Duration::from_secs(1)).await;
/// clock.sleep(Duration::from_secs(2)).await;
///
/// assert_eq!(clock.sleeps(), &[Duration::from_secs(1), Duration::from_secs(2)]);
/// assert_eq!(clock.total_slept(), Duration::from_secs(3));
/// # })
/// ```
#[derive(Debug, Clone, Default)]
pub struct RecordingClock {
    sleeps: Vec<Duration>,
}

impl RecordingClock {
    /// Creates a clock that has recorded nothing.
    ///
    /// # Examples
    ///
    /// ```
    /// use retry_policy::RecordingClock;
    ///
    /// assert!(RecordingClock::new().sleeps().is_empty());
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns every sleep requested, in order.
    ///
    /// # Examples
    ///
    /// ```
    /// use retry_policy::RecordingClock;
    ///
    /// assert_eq!(RecordingClock::new().sleeps().len(), 0);
    /// ```
    pub fn sleeps(&self) -> &[Duration] {
        &self.sleeps
    }

    /// Returns the sum of every requested sleep.
    ///
    /// This is the wall time a real clock would have spent, which is what a
    /// test asserting "this policy cannot stall for more than a minute" cares
    /// about.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use retry_policy::RecordingClock;
    ///
    /// assert_eq!(RecordingClock::new().total_slept(), Duration::ZERO);
    /// ```
    pub fn total_slept(&self) -> Duration {
        self.sleeps.iter().sum()
    }
}

impl Clock for RecordingClock {
    async fn sleep(&mut self, duration: Duration) {
        self.sleeps.push(duration);
    }
}

/// A [`Clock`] backed by `tokio::time::sleep`.
///
/// Honors a paused tokio clock, so a consumer's own tests can use
/// `#[tokio::test(start_paused = true)]` and still not spend real time.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use retry_policy::{Clock, TokioClock};
///
/// # tokio_test::block_on(async {
/// let mut clock = TokioClock;
/// clock.sleep(Duration::ZERO).await;
/// # })
/// ```
#[cfg(feature = "tokio")]
#[derive(Debug, Clone, Copy, Default)]
pub struct TokioClock;

#[cfg(feature = "tokio")]
impl Clock for TokioClock {
    async fn sleep(&mut self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_cycles() {
        let mut rng = SequenceRandomness::new(vec![0.1, 0.2, 0.3]);
        let drawn: Vec<f64> = (0..7).map(|_| rng.next_f64()).collect();
        assert_eq!(drawn, vec![0.1, 0.2, 0.3, 0.1, 0.2, 0.3, 0.1]);
    }

    #[test]
    fn empty_sequence_yields_zero_forever() {
        // Must not panic: a caller passing an empty sequence would otherwise
        // take down the retry loop rather than the test.
        let mut rng = SequenceRandomness::new(vec![]);
        for _ in 0..5 {
            assert_eq!(rng.next_f64(), 0.0);
        }
    }

    #[test]
    fn fixed_always_returns_the_same_value() {
        let mut rng = SequenceRandomness::fixed(0.42);
        assert_eq!(rng.next_f64(), 0.42);
        assert_eq!(rng.next_f64(), 0.42);
    }

    #[test]
    fn process_randomness_stays_in_range() {
        let mut rng = ProcessRandomness::seeded(12345);
        for _ in 0..10_000 {
            let r = rng.next_f64();
            assert!((0.0..1.0).contains(&r), "{r} out of range");
        }
    }

    #[test]
    fn process_randomness_varies_between_draws() {
        // A source stuck on one value would make jitter useless.
        let mut rng = ProcessRandomness::seeded(1);
        let draws: Vec<f64> = (0..50).map(|_| rng.next_f64()).collect();
        let first = draws[0];
        assert!(draws.iter().any(|d| *d != first), "no variation");
    }

    #[test]
    fn process_randomness_spreads_across_the_range() {
        // Not a statistical test, just a guard against a source that clusters
        // hard enough to defeat the purpose of jitter.
        let mut rng = ProcessRandomness::seeded(99);
        let draws: Vec<f64> = (0..1000).map(|_| rng.next_f64()).collect();
        assert!(
            draws.iter().any(|d| *d < 0.25),
            "nothing in the low quarter"
        );
        assert!(
            draws.iter().any(|d| *d > 0.75),
            "nothing in the high quarter"
        );
    }

    #[test]
    fn process_randomness_is_reproducible_from_a_seed() {
        let a: Vec<f64> = {
            let mut r = ProcessRandomness::seeded(42);
            (0..5).map(|_| r.next_f64()).collect()
        };
        let b: Vec<f64> = {
            let mut r = ProcessRandomness::seeded(42);
            (0..5).map(|_| r.next_f64()).collect()
        };
        assert_eq!(a, b);
    }

    #[test]
    fn a_zero_seed_does_not_collapse_to_zero() {
        // xorshift is a fixed point at zero; the constructor must avoid it.
        let mut rng = ProcessRandomness::seeded(0);
        let draws: Vec<f64> = (0..10).map(|_| rng.next_f64()).collect();
        assert!(draws.iter().any(|d| *d != 0.0), "collapsed to zero");
    }

    #[test]
    fn independently_constructed_sources_differ() {
        // The property that actually matters: two concurrent callers each
        // building their own source must not draw the same sequence.
        let mut a = ProcessRandomness::new();
        let mut b = ProcessRandomness::new();
        let left: Vec<f64> = (0..8).map(|_| a.next_f64()).collect();
        let right: Vec<f64> = (0..8).map(|_| b.next_f64()).collect();
        assert_ne!(left, right);
    }

    #[tokio::test]
    async fn recording_clock_records_in_order_without_waiting() {
        let mut clock = RecordingClock::new();
        clock.sleep(Duration::from_millis(10)).await;
        clock.sleep(Duration::from_millis(20)).await;
        clock.sleep(Duration::ZERO).await;

        assert_eq!(
            clock.sleeps(),
            &[
                Duration::from_millis(10),
                Duration::from_millis(20),
                Duration::ZERO
            ]
        );
        assert_eq!(clock.total_slept(), Duration::from_millis(30));
    }
}
