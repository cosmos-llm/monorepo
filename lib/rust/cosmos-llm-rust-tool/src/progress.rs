//! Progress reporting for long-running agent runs.

use std::io::{IsTerminal, Write};
use std::sync::Mutex;

/// Reports the progress of a long-running agent run to a stream.
///
/// An agent loop that thinks for two minutes between tool calls is
/// indistinguishable from a hung process. This gives a run somewhere to say
/// what it is doing without every caller inventing its own `eprintln!`.
///
/// Everything goes to stderr, so a CLI writing JSON to stdout stays pipeable
/// while still showing progress on a terminal.
///
/// A live terminal gets a single line updated in place; a redirected stream
/// gets one line per update, since carriage returns are noise in a log file.
/// [`Progress::silent`] discards everything, so library code can hold a
/// reporter unconditionally rather than an `Option`.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_tool::Progress;
///
/// let progress = Progress::silent();
/// progress.step("researching");
/// progress.update("searching corpus");
/// progress.finish(Some("done"));
/// ```
#[derive(Debug)]
pub struct Progress {
    sink: Sink,
    /// Whether an in-place status line is waiting to be erased.
    dirty: Mutex<bool>,
}

/// Where a reporter writes, and how.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sink {
    /// A terminal: rewrite one status line in place.
    Terminal,
    /// A pipe or file: one line per update.
    Stream,
    /// Discard everything.
    Silent,
}

impl Default for Progress {
    fn default() -> Self {
        Self::new()
    }
}

impl Progress {
    /// Creates a reporter writing to stderr, detecting whether it is a
    /// terminal.
    pub fn new() -> Self {
        let sink = if std::io::stderr().is_terminal() {
            Sink::Terminal
        } else {
            Sink::Stream
        };
        Self {
            sink,
            dirty: Mutex::new(false),
        }
    }

    /// Creates a reporter that writes nothing.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::Progress;
    ///
    /// let quiet = Progress::silent();
    /// quiet.step("this is not printed");
    /// ```
    pub fn silent() -> Self {
        Self {
            sink: Sink::Silent,
            dirty: Mutex::new(false),
        }
    }

    /// Creates a reporter that always writes one line per call, regardless of
    /// whether stderr is a terminal. Useful for a `--no-tty` flag.
    pub fn plain() -> Self {
        Self {
            sink: Sink::Stream,
            dirty: Mutex::new(false),
        }
    }

    /// Returns `true` if this reporter discards everything.
    pub fn is_silent(&self) -> bool {
        self.sink == Sink::Silent
    }

    /// Announces a new step on its own line.
    pub fn step(&self, message: &str) {
        if self.sink == Sink::Silent {
            return;
        }
        let mut out = std::io::stderr();
        self.clear(&mut out);
        let _ = writeln!(out, "{message}");
        let _ = out.flush();
    }

    /// Replaces the current status line, or logs it when not on a terminal.
    pub fn update(&self, message: &str) {
        match self.sink {
            Sink::Silent => (),
            Sink::Terminal => {
                let mut out = std::io::stderr();
                let _ = write!(out, "\r\x1b[K{message}");
                let _ = out.flush();
                *self.dirty_lock() = true;
            }
            Sink::Stream => {
                let mut out = std::io::stderr();
                let _ = writeln!(out, "{message}");
                let _ = out.flush();
            }
        }
    }

    /// Ends the current status line, optionally leaving a final message.
    pub fn finish(&self, message: Option<&str>) {
        if self.sink == Sink::Silent {
            return;
        }
        let mut out = std::io::stderr();
        self.clear(&mut out);
        if let Some(text) = message {
            let _ = writeln!(out, "{text}");
        }
        let _ = out.flush();
    }

    /// Erases a pending in-place status line, if any.
    fn clear(&self, out: &mut impl Write) {
        let mut dirty = self.dirty_lock();
        if *dirty {
            let _ = write!(out, "\r\x1b[K");
            *dirty = false;
        }
    }

    /// Takes the dirty flag, recovering from a poisoned mutex — a failed write
    /// is not a reason to panic a run that is otherwise fine.
    fn dirty_lock(&self) -> std::sync::MutexGuard<'_, bool> {
        self.dirty.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silent_reporter_accepts_every_call() {
        let progress = Progress::silent();
        progress.step("a");
        progress.update("b");
        progress.finish(Some("c"));
        progress.finish(None);
        assert!(progress.is_silent());
    }

    #[test]
    fn plain_reporter_is_not_silent() {
        assert!(!Progress::plain().is_silent());
    }

    #[test]
    fn new_reporter_is_not_silent() {
        // Whether it picks Terminal or Stream depends on how the test harness
        // was invoked; either way it reports something.
        assert!(!Progress::new().is_silent());
    }

    #[test]
    fn clear_is_a_no_op_when_nothing_is_pending() {
        let progress = Progress::plain();
        let mut buffer: Vec<u8> = Vec::new();
        progress.clear(&mut buffer);
        assert!(buffer.is_empty());
    }

    #[test]
    fn clear_erases_a_pending_line_once() {
        let progress = Progress::plain();
        *progress.dirty_lock() = true;

        let mut buffer: Vec<u8> = Vec::new();
        progress.clear(&mut buffer);
        assert_eq!(String::from_utf8(buffer).unwrap(), "\r\x1b[K");

        // A second clear writes nothing: the flag was reset.
        let mut again: Vec<u8> = Vec::new();
        progress.clear(&mut again);
        assert!(again.is_empty());
    }
}

// Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
