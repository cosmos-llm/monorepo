//! What a running tool is handed besides its arguments (feature `async`).

use std::sync::{Arc, Mutex};

use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::session::Session;

/// Where a long-running tool reports output while it works.
///
/// The agent loop turns each chunk into an
/// [`AgentEvent::ToolOutputDelta`](crate::AgentEvent::ToolOutputDelta) for the
/// call. Chunks are for the UI only: the model still sees just the tool's
/// final result, so a shell tool streams lines here and returns the (limited)
/// transcript when it exits.
///
/// A sink is valid for the life of the call. The loop stops listening when the
/// handler returns, so a background job that outlives its call must not report
/// through a clone of the sink; give it a job id and let the model poll it with
/// a tool instead. Sends after the call ended are dropped.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_tool::ProgressSink;
///
/// // A detached sink swallows everything, so a handler can use it blindly.
/// let sink = ProgressSink::none();
/// assert!(!sink.try_send("ignored"));
/// ```
#[derive(Debug, Clone)]
pub struct ProgressSink {
    tx: Option<mpsc::Sender<String>>,
}

impl ProgressSink {
    /// A sink connected to nothing.
    pub fn none() -> Self {
        Self { tx: None }
    }

    #[cfg_attr(not(feature = "agent"), allow(dead_code))]
    pub(crate) fn new(tx: mpsc::Sender<String>) -> Self {
        Self { tx: Some(tx) }
    }

    /// Returns `true` if something is listening.
    pub fn is_connected(&self) -> bool {
        self.tx.as_ref().is_some_and(|tx| !tx.is_closed())
    }

    /// Sends a chunk, waiting if the consumer is behind (backpressure).
    pub async fn send(&self, chunk: impl Into<String>) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(chunk.into()).await;
        }
    }

    /// Sends a chunk without waiting. Returns `false` if it was dropped
    /// because the consumer is behind or gone.
    pub fn try_send(&self, chunk: impl Into<String>) -> bool {
        self.tx
            .as_ref()
            .is_some_and(|tx| tx.try_send(chunk.into()).is_ok())
    }
}

/// Per-call context for a handler registered with
/// [`ToolDefinition::context_handler`](crate::ToolDefinition::context_handler).
///
/// Holds live handles, so it is not an IR; what it produces is: progress goes
/// out as `ToolOutputDelta` events and [`ToolContext::set_display`] ends up in
/// [`ToolOutcome::display`](crate::ToolOutcome::display).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ToolContext {
    /// The model's id for this call.
    pub call_id: String,
    /// Fires when the user interrupts. Long handlers must await it (or poll
    /// [`CancellationToken::is_cancelled`]) and stop, killing any child
    /// processes; the loop only waits a short grace period.
    pub cancel: CancellationToken,
    /// Streams output to the UI while the call runs.
    pub progress: ProgressSink,
    /// The run's shared session, when the loop has one.
    pub session: Option<Arc<Session>>,
    display: Arc<Mutex<Option<Value>>>,
}

impl ToolContext {
    /// A context attached to nothing: never cancelled, progress discarded.
    ///
    /// What [`Executor::execute_async`](crate::Executor::execute_async) passes,
    /// and what tests use.
    pub fn detached(call_id: impl Into<String>) -> Self {
        Self::new(
            call_id,
            CancellationToken::new(),
            ProgressSink::none(),
            None,
        )
    }

    pub(crate) fn new(
        call_id: impl Into<String>,
        cancel: CancellationToken,
        progress: ProgressSink,
        session: Option<Arc<Session>>,
    ) -> Self {
        Self {
            call_id: call_id.into(),
            cancel,
            progress,
            session,
            display: Arc::new(Mutex::new(None)),
        }
    }

    /// Sets structured data for the UI, such as a diff or an exit code.
    ///
    /// Not shown to the model. Each tool crate documents the IR it puts here.
    pub fn set_display(&self, value: Value) {
        *self.display.lock().unwrap_or_else(|e| e.into_inner()) = Some(value);
    }

    #[cfg_attr(not(feature = "agent"), allow(dead_code))]
    pub(crate) fn take_display(&self) -> Option<Value> {
        self.display
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }
}
