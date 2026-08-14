//! Per-run state shared between an agent loop and the tools it dispatches.

use std::collections::HashMap;
use std::sync::Mutex;

/// Per-run mutable state shared by a loop and the tools it dispatches: named
/// call budgets, a finish flag, and free-form notes.
///
/// A tool handler holds an `Arc<Session>` and consults it before doing
/// anything expensive. The caps live here rather than in a prompt because a
/// prompt is a request and this is an invariant — a model asked nicely to stop
/// searching will, eventually, search again.
///
/// # Interior mutability
///
/// Every method takes `&self`, not `&mut self`. Tool handlers are
/// `Fn + Send + Sync` (see [`crate::Handler`]), so a handler that closed over a
/// `&mut Session` would not compile, and one that owned a `RefCell` would not
/// be `Sync`. A `Mutex` inside is the shape that lets several tools share one
/// session across an async dispatch without the caller threading anything.
///
/// # Budgets are named, not enumerated
///
/// The crate cannot know what a given agent spends. Rather than hardcode
/// counters for searches and file opens, a session takes caps keyed by
/// whatever the caller wants to meter. An unlisted key is unmetered: it is
/// still counted, and [`Session::allowed`] always returns `true` for it.
///
/// # Exhaustion is a message, not an error
///
/// [`Session::exhausted_message`] exists because the right response to a spent
/// budget is to tell the model, in the tool result, that it has run out and
/// should wrap up. Returning `Err` instead would abort a run that is mostly
/// successful and discard whatever the agent had already gathered.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_tool::Session;
///
/// let session = Session::with_budgets([("search", 2)]);
///
/// assert!(session.allowed("search"));
/// session.consume("search");
/// session.consume("search");
/// assert!(!session.allowed("search"));
///
/// // An unlisted key is never exhausted.
/// assert!(session.allowed("anything-else"));
/// ```
#[derive(Debug, Default)]
pub struct Session {
    budgets: HashMap<String, u32>,
    state: Mutex<State>,
}

/// The mutable half, kept behind one lock so a consume-then-check cannot
/// interleave with another thread's.
#[derive(Debug, Default)]
struct State {
    counts: HashMap<String, u32>,
    notes: Vec<String>,
    finished: bool,
}

impl Session {
    /// Creates a session with no budgets. Every key is unmetered.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::Session;
    ///
    /// let session = Session::new();
    /// assert!(session.allowed("anything"));
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a session metering the given keys.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::Session;
    ///
    /// let session = Session::with_budgets([("search", 40), ("open", 25)]);
    /// assert_eq!(session.remaining("open"), Some(25));
    /// ```
    pub fn with_budgets<I, K>(budgets: I) -> Self
    where
        I: IntoIterator<Item = (K, u32)>,
        K: Into<String>,
    {
        Self {
            budgets: budgets
                .into_iter()
                .map(|(key, cap)| (key.into(), cap))
                .collect(),
            state: Mutex::new(State::default()),
        }
    }

    /// Returns `true` when another call against `key` is within budget.
    ///
    /// Unmetered keys are always allowed.
    pub fn allowed(&self, key: &str) -> bool {
        match self.budgets.get(key) {
            None => true,
            Some(cap) => self.count(key) < *cap,
        }
    }

    /// Records one use of `key` and returns the new count.
    ///
    /// Counting past the cap is deliberate: a run summary saying an agent tried
    /// to search sixty times against a cap of forty is more useful than one
    /// that stops counting at forty.
    pub fn consume(&self, key: &str) -> u32 {
        self.consume_n(key, 1)
    }

    /// Records `n` uses of `key` and returns the new count.
    pub fn consume_n(&self, key: &str, n: u32) -> u32 {
        let mut state = self.lock();
        let entry = state.counts.entry(key.to_owned()).or_insert(0);
        *entry += n;
        *entry
    }

    /// Returns how many times `key` has been consumed.
    pub fn count(&self, key: &str) -> u32 {
        self.lock().counts.get(key).copied().unwrap_or(0)
    }

    /// Returns how many uses of `key` remain, or `None` when unmetered.
    ///
    /// Saturates at zero rather than wrapping, so a key consumed past its cap
    /// reports no remaining budget instead of a very large one.
    pub fn remaining(&self, key: &str) -> Option<u32> {
        self.budgets
            .get(key)
            .map(|cap| cap.saturating_sub(self.count(key)))
    }

    /// Returns the text to hand back to the model when a budget is spent.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::Session;
    ///
    /// let session = Session::with_budgets([("search", 1)]);
    /// session.consume("search");
    /// assert!(session.exhausted_message("search").contains("search budget exhausted"));
    /// ```
    pub fn exhausted_message(&self, key: &str) -> String {
        format!(
            "{} budget exhausted ({} used). Record what you have and finish.",
            key,
            self.count(key)
        )
    }

    /// Returns `true` once a tool has signalled that the run is over.
    pub fn finished(&self) -> bool {
        self.lock().finished
    }

    /// Signals that the run is over. A `done` tool calls this; the loop checks
    /// it after each turn and stops.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::Session;
    ///
    /// let session = Session::new();
    /// session.finish(Some("queued 4 documents"));
    /// assert!(session.finished());
    /// assert_eq!(session.notes(), vec!["queued 4 documents".to_string()]);
    /// ```
    pub fn finish(&self, note: Option<&str>) {
        let mut state = self.lock();
        if let Some(text) = note.filter(|t| !t.is_empty()) {
            state.notes.push(text.to_owned());
        }
        state.finished = true;
    }

    /// Records a note without ending the run.
    pub fn note(&self, note: &str) {
        self.lock().notes.push(note.to_owned());
    }

    /// Returns the notes recorded so far.
    pub fn notes(&self) -> Vec<String> {
        self.lock().notes.clone()
    }

    /// Returns a snapshot of every key's count, for logging a finished run.
    pub fn counts(&self) -> HashMap<String, u32> {
        self.lock().counts.clone()
    }

    /// Returns the configured caps.
    pub fn budgets(&self) -> &HashMap<String, u32> {
        &self.budgets
    }

    /// Takes the state lock, recovering from a poisoned mutex.
    ///
    /// A panic inside a tool handler must not turn every later budget check
    /// into a panic of its own: the run is already degraded, and the counters
    /// are plain integers that a partial update cannot corrupt into anything
    /// unsound.
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unmetered_key_is_always_allowed() {
        let session = Session::new();
        for _ in 0..20 {
            session.consume("anything");
        }
        assert!(session.allowed("anything"));
        assert_eq!(session.remaining("anything"), None);
        assert_eq!(session.count("anything"), 20);
    }

    #[test]
    fn metered_key_is_refused_once_the_cap_is_reached() {
        let session = Session::with_budgets([("search", 2)]);
        assert!(session.allowed("search"));
        session.consume("search");
        assert!(session.allowed("search"));
        session.consume("search");
        assert!(!session.allowed("search"));
    }

    #[test]
    fn remaining_counts_down_and_saturates_at_zero() {
        let session = Session::with_budgets([("open", 2)]);
        assert_eq!(session.remaining("open"), Some(2));
        session.consume("open");
        assert_eq!(session.remaining("open"), Some(1));
        session.consume_n("open", 5);
        assert_eq!(session.remaining("open"), Some(0));
    }

    #[test]
    fn counts_continue_past_the_cap() {
        let session = Session::with_budgets([("search", 1)]);
        for _ in 0..3 {
            session.consume("search");
        }
        assert_eq!(session.count("search"), 3);
        assert_eq!(session.remaining("search"), Some(0));
    }

    #[test]
    fn exhausted_message_names_the_key_and_the_count() {
        let session = Session::with_budgets([("search", 1)]);
        session.consume("search");
        let message = session.exhausted_message("search");
        assert!(message.contains("search budget exhausted"));
        assert!(message.contains("1 used"));
    }

    #[test]
    fn finish_sets_the_flag_and_records_a_note() {
        let session = Session::new();
        assert!(!session.finished());
        session.finish(Some("all set"));
        assert!(session.finished());
        assert_eq!(session.notes(), vec!["all set".to_string()]);
    }

    #[test]
    fn finish_without_a_note_records_nothing() {
        let session = Session::new();
        session.finish(None);
        assert!(session.finished());
        assert!(session.notes().is_empty());
        // An empty note is not a note either.
        let other = Session::new();
        other.finish(Some(""));
        assert!(other.notes().is_empty());
    }

    #[test]
    fn counts_snapshot_reports_every_key() {
        let session = Session::with_budgets([("search", 5)]);
        session.consume_n("search", 2);
        session.consume("open");
        let counts = session.counts();
        assert_eq!(counts.get("search"), Some(&2));
        assert_eq!(counts.get("open"), Some(&1));
    }

    #[test]
    fn shared_across_threads() {
        use std::sync::Arc;

        let session = Arc::new(Session::with_budgets([("search", 100)]));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let session = Arc::clone(&session);
                std::thread::spawn(move || {
                    for _ in 0..10 {
                        session.consume("search");
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        assert_eq!(session.count("search"), 80);
    }
}

// Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
