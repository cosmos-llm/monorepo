//! The interactive end of the approver chain (210).
//!
//! The loop calls [`Approver::check`] and waits. [`InteractiveApprover`] parks
//! that call on a oneshot keyed by `call_id` until the frontend answers with
//! `Op::Approve`. It sits behind the policy engine
//! ([`PolicyApprover`](crate::PolicyApprover)): only calls the policy could
//! not decide reach it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use cosmos_llm_tool::{ApprovalRequest, Approver, Decision};
use futures_util::future::BoxFuture;
use tokio::sync::{mpsc, oneshot};

/// Answers that arrived before their question, kept at most this many.
const MAX_EARLY: usize = 64;

#[derive(Default)]
struct State {
    pending: HashMap<String, oneshot::Sender<Decision>>,
    early: HashMap<String, Decision>,
}

/// Waits for the frontend's answer to each approval question.
///
/// An `Approve` that beats its question (the frontend answered an event it
/// saw through another path) is kept and used when the question arrives.
pub struct InteractiveApprover {
    state: Mutex<State>,
    asked: mpsc::UnboundedSender<ApprovalRequest>,
}

impl std::fmt::Debug for InteractiveApprover {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InteractiveApprover")
            .field("pending", &self.pending())
            .finish()
    }
}

struct Cleanup<'a> {
    approver: &'a InteractiveApprover,
    call_id: &'a str,
}

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        lock(&self.approver.state).pending.remove(self.call_id);
    }
}

fn lock(m: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl InteractiveApprover {
    /// Creates the approver and the stream of questions it has put to the
    /// user (the core turns these into `Event::ApprovalRequested`).
    pub fn new() -> (Arc<Self>, mpsc::UnboundedReceiver<ApprovalRequest>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            Arc::new(Self {
                state: Mutex::new(State::default()),
                asked: tx,
            }),
            rx,
        )
    }

    /// Delivers an answer. Returns whether a question was waiting (an answer
    /// with no question yet is kept for it and also returns `false`).
    pub fn resolve(&self, call_id: &str, decision: Decision) -> bool {
        let mut st = lock(&self.state);
        match st.pending.remove(call_id) {
            Some(tx) => tx.send(decision).is_ok(),
            None => {
                if st.early.len() < MAX_EARLY {
                    st.early.insert(call_id.to_owned(), decision);
                }
                false
            }
        }
    }

    /// The calls waiting for an answer.
    pub fn pending(&self) -> Vec<String> {
        let mut v: Vec<_> = lock(&self.state).pending.keys().cloned().collect();
        v.sort();
        v
    }
}

impl Approver for InteractiveApprover {
    fn check<'a>(&'a self, req: &'a ApprovalRequest) -> BoxFuture<'a, Decision> {
        Box::pin(async move {
            let rx = {
                let mut st = lock(&self.state);
                if let Some(d) = st.early.remove(&req.call_id) {
                    return d;
                }
                let (tx, rx) = oneshot::channel();
                st.pending.insert(req.call_id.clone(), tx);
                rx
            };
            let _cleanup = Cleanup {
                approver: self,
                call_id: &req.call_id,
            };
            if self.asked.send(req.clone()).is_err() {
                return Decision::Abort;
            }
            rx.await.unwrap_or(Decision::Abort)
        })
    }
}
