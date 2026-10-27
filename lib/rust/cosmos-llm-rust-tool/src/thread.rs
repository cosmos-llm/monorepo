//! Threads: conversations that outlive one run.
//!
//! [`AgentLoop::run_with`] is a stateless turn engine over a history it is
//! lent. A [`Thread`] owns that history between runs, queues input typed while
//! the agent is busy, and lets the history be rewritten in place (compaction,
//! "edit and resend").

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use cosmos_llm::{ContentBlock, CosmosError, Message, Role};
use serde::{Deserialize, Serialize};

use crate::agent::{AgentLoop, Completer, RunHandles, RunOutcome, StopReason, TokenUsage};

/// Identifies a thread.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ThreadId(pub String);

impl<S: Into<String>> From<S> for ThreadId {
    fn from(s: S) -> Self {
        Self(s.into())
    }
}

/// What a thread is doing right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ThreadStatus {
    /// Nothing is running; input starts a run.
    #[default]
    Idle,
    /// A run is in progress; input is queued.
    Running,
    /// A run is paused on an approval prompt.
    AwaitingApproval,
}

/// A shared, observable [`ThreadStatus`].
///
/// The loop sets it as a run moves between phases; a frontend on another task
/// reads it.
#[derive(Debug, Clone, Default)]
pub struct StatusCell(Arc<Mutex<ThreadStatus>>);

impl StatusCell {
    /// The current status.
    pub fn get(&self) -> ThreadStatus {
        *self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Sets the status.
    pub fn set(&self, status: ThreadStatus) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = status;
    }
}

/// One message from the user.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_tool::UserInput;
///
/// let input = UserInput::from("fix the build");
/// assert_eq!(input.text(), "fix the build");
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct UserInput {
    /// The message content: text, plus images if any.
    pub content: Vec<ContentBlock>,
}

impl UserInput {
    /// Input from blocks.
    pub fn from_blocks(content: Vec<ContentBlock>) -> Self {
        Self { content }
    }

    /// The text blocks, concatenated.
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl From<&str> for UserInput {
    fn from(s: &str) -> Self {
        Self {
            content: vec![ContentBlock::text(s)],
        }
    }
}

impl From<String> for UserInput {
    fn from(s: String) -> Self {
        Self {
            content: vec![ContentBlock::text(s)],
        }
    }
}

/// Input waiting for the next safe point.
///
/// Cheap to clone; every clone is the same queue. A frontend keeps one and
/// pushes while the thread runs. The loop drains it between tool results and
/// the next model call, so the model sees the new message on its next step
/// instead of a second run starting.
#[derive(Debug, Clone, Default)]
pub struct InputQueue(Arc<Mutex<VecDeque<UserInput>>>);

impl InputQueue {
    /// An empty queue.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<UserInput>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Queues input.
    pub fn push(&self, input: impl Into<UserInput>) {
        self.lock().push_back(input.into());
    }

    /// Removes and returns everything queued, oldest first.
    pub fn drain(&self) -> Vec<UserInput> {
        self.lock().drain(..).collect()
    }

    /// Number of queued inputs.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Returns `true` if nothing is queued.
    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    /// Drops everything queued.
    pub fn clear(&self) {
        self.lock().clear();
    }
}

/// Merges queued inputs into one user message, so the history never gains
/// consecutive user messages from a burst of typing.
pub(crate) fn merge_inputs(inputs: Vec<UserInput>) -> Message {
    let mut content = Vec::new();
    for input in inputs {
        if !content.is_empty() {
            content.push(ContentBlock::text("\n"));
        }
        content.extend(input.content);
    }
    Message::user_blocks(content)
}

/// Why a history cannot be sent to a provider.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum HistoryError {
    /// An assistant `ToolUse` has no matching `ToolResult` before the next
    /// message that is not a result.
    #[error("message {index}: tool call {id} has no result")]
    DanglingToolUse {
        /// Index of the assistant message.
        index: usize,
        /// The call's id.
        id: String,
    },
    /// A `ToolResult` answers no call from the preceding assistant message.
    #[error("message {index}: tool result {id} answers no call")]
    OrphanToolResult {
        /// Index of the result message.
        index: usize,
        /// The id it names.
        id: String,
    },
    /// A message cannot be a truncation point.
    #[error("message {index} is not a user message")]
    NotUserMessage {
        /// The offending index.
        index: usize,
    },
}

/// Checks the invariants every provider relies on.
///
/// 1. Every assistant `ToolUse` is followed by a `ToolResult` with the same id
///    before any other message.
/// 2. Every `ToolResult` answers a `ToolUse` of the assistant message before it.
///
/// Consecutive user messages are not checked: provider mapping merges them.
/// The loop calls this in debug builds before each request, which catches
/// compaction and interrupt bugs where they are cheap to diagnose.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm::{ContentBlock, Message};
/// use cosmos_llm_tool::validate_history;
/// use serde_json::json;
///
/// let call = Message::assistant_blocks(vec![ContentBlock::ToolUse {
///     id: "c1".into(), name: "echo".into(), input: json!({}),
/// }]);
/// assert!(validate_history(&[Message::user("hi"), call.clone()]).is_err());
/// assert!(validate_history(&[Message::user("hi"), call, Message::tool_result("c1", "ok")]).is_ok());
/// ```
///
/// # Errors
///
/// Returns the first violation found.
pub fn validate_history(msgs: &[Message]) -> Result<(), HistoryError> {
    let mut open: Vec<(usize, String)> = Vec::new();
    for (index, msg) in msgs.iter().enumerate() {
        let results: Vec<&str> = msg
            .content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::ToolResult { tool_use_id, .. } => Some(tool_use_id.as_str()),
                _ => None,
            })
            .collect();

        if results.is_empty() {
            if let Some((at, id)) = open.first() {
                return Err(HistoryError::DanglingToolUse {
                    index: *at,
                    id: id.clone(),
                });
            }
            if msg.role == Role::Assistant {
                open = msg.tool_calls().map(|c| (index, c.id)).collect();
            }
        } else {
            for id in results {
                match open.iter().position(|(_, open_id)| open_id == id) {
                    Some(pos) => {
                        open.remove(pos);
                    }
                    None => {
                        return Err(HistoryError::OrphanToolResult {
                            index,
                            id: id.to_owned(),
                        })
                    }
                }
            }
        }
    }
    match open.into_iter().next() {
        Some((index, id)) => Err(HistoryError::DanglingToolUse { index, id }),
        None => Ok(()),
    }
}

/// A conversation that outlives one run.
///
/// Holds the history, the queue of input typed while the agent works, and the
/// running token totals. The tool set and system prompt live on the
/// [`AgentLoop`] you pass to each call, so swapping either between turns (a
/// mode change, a newly enabled tool) is just passing a different loop.
///
/// # Driving a thread
///
/// ```rust,no_run
/// # async fn demo(agent: cosmos_llm_tool::AgentLoop, client: &cosmos_llm::Client) {
/// use cosmos_llm_tool::{event_channel, RunHandles, Thread};
///
/// let mut thread = Thread::new("main");
/// let queue = thread.queue(); // hand this to the UI task
/// let (tx, mut rx) = event_channel(256);
/// let handles = RunHandles::new().events(tx);
/// tokio::spawn(async move { while let Some(_event) = rx.recv().await { /* render */ } });
///
/// thread.send(&agent, client, "fix the failing test", &handles).await.unwrap();
/// // Typed while it was running? `queue.push("also update the docs")` was
/// // delivered at the next safe point, or runs next inside `send`.
/// thread.send(&agent, client, "now commit it", &handles).await.unwrap();
/// # }
/// ```
#[derive(Debug)]
pub struct Thread {
    id: ThreadId,
    history: Vec<Message>,
    queue: InputQueue,
    usage: TokenUsage,
    status: StatusCell,
}

impl Thread {
    /// An empty thread.
    pub fn new(id: impl Into<ThreadId>) -> Self {
        Self::with_history(id, Vec::new())
    }

    /// A thread resuming from a saved history.
    pub fn with_history(id: impl Into<ThreadId>, history: Vec<Message>) -> Self {
        Self {
            id: id.into(),
            history,
            queue: InputQueue::new(),
            usage: TokenUsage::default(),
            status: StatusCell::default(),
        }
    }

    /// The thread's id.
    pub fn id(&self) -> &ThreadId {
        &self.id
    }

    /// The history, oldest first.
    pub fn history(&self) -> &[Message] {
        &self.history
    }

    /// The history, for in-place rewriting such as compaction. Keep it valid:
    /// see [`validate_history`].
    pub fn history_mut(&mut self) -> &mut Vec<Message> {
        &mut self.history
    }

    /// Tokens used across every run of this thread.
    pub fn usage(&self) -> &TokenUsage {
        &self.usage
    }

    /// The thread's status, shared with the running loop.
    pub fn status(&self) -> ThreadStatus {
        self.status.get()
    }

    /// A handle to the shared status, for a frontend task.
    pub fn status_cell(&self) -> StatusCell {
        self.status.clone()
    }

    /// A handle to the input queue. Push to it from any task, at any time.
    pub fn queue(&self) -> InputQueue {
        self.queue.clone()
    }

    /// Drops the user message at `index` and everything after it, returning
    /// that message so a frontend can offer "edit and resend".
    ///
    /// File changes made after that point are not undone here.
    ///
    /// # Errors
    ///
    /// [`HistoryError::NotUserMessage`] when `index` is out of range or is not
    /// a plain user message (tool results carry the user role on some
    /// providers but are not valid truncation points).
    pub fn truncate_to(&mut self, index: usize) -> Result<Message, HistoryError> {
        let ok = self
            .history
            .get(index)
            .is_some_and(|m| m.role == Role::User && !m.is_tool_result());
        if !ok {
            return Err(HistoryError::NotUserMessage { index });
        }
        let dropped = self.history[index].clone();
        self.history.truncate(index);
        Ok(dropped)
    }

    /// Appends `input` and runs until the model has nothing more to do.
    ///
    /// Input pushed to [`Thread::queue`] while this runs is delivered at the
    /// next safe point. Input that arrives too late for that (after the final
    /// answer, or with the step cap spent) starts a further run before `send`
    /// returns, unless the run was interrupted or aborted, in which case it
    /// stays queued for [`Thread::run_queued`] or [`InputQueue::clear`].
    ///
    /// Overrides `handles.steer` and `handles.status` with the thread's own.
    ///
    /// # Errors
    ///
    /// Whatever the client returns. The history keeps its progress.
    pub async fn send(
        &mut self,
        agent: &AgentLoop,
        client: &dyn Completer,
        input: impl Into<UserInput>,
        handles: &RunHandles,
    ) -> Result<RunOutcome, CosmosError> {
        self.queue.push(input);
        self.run_queued(agent, client, handles).await
    }

    /// Runs whatever is queued.
    ///
    /// With nothing queued it runs only if the history ends in a user message
    /// (a resumed thread, or a run that was interrupted before the model
    /// answered); otherwise it returns an empty, finished outcome.
    ///
    /// # Errors
    ///
    /// Whatever the client returns.
    pub async fn run_queued(
        &mut self,
        agent: &AgentLoop,
        client: &dyn Completer,
        handles: &RunHandles,
    ) -> Result<RunOutcome, CosmosError> {
        let mut handles = handles.clone();
        handles.steer = Some(self.queue.clone());
        handles.status = Some(self.status.clone());

        let mut total = TokenUsage::default();
        let mut steps = 0;
        let mut last: Option<RunOutcome> = None;

        loop {
            let pending = self.queue.drain();
            if pending.is_empty() {
                let awaiting_reply = self
                    .history
                    .last()
                    .is_some_and(|m| m.role == Role::User && !m.is_tool_result());
                if last.is_some() || !awaiting_reply {
                    break;
                }
            } else {
                self.history.push(merge_inputs(pending));
            }

            self.status.set(ThreadStatus::Running);
            let result = agent.run_with(client, &mut self.history, &handles).await;
            self.status.set(ThreadStatus::Idle);
            let outcome = result?;

            total.add(&outcome.usage);
            steps += outcome.steps_taken;
            let stop = matches!(
                outcome.reason,
                StopReason::Interrupted | StopReason::Aborted
            );
            last = Some(outcome);
            if stop {
                break;
            }
        }

        self.usage.add(&total);
        let mut outcome = last.unwrap_or_else(|| RunOutcome {
            text: String::new(),
            steps_taken: 0,
            reason: StopReason::Finished,
            usage: TokenUsage::default(),
            messages: self.history.clone(),
        });
        outcome.usage = total;
        outcome.steps_taken = steps;
        outcome.messages = self.history.clone();
        Ok(outcome)
    }
}
