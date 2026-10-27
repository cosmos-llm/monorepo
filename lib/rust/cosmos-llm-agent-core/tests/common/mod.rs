#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use cosmos_llm::providers::mock::MockProvider;
use cosmos_llm::Client;
use cosmos_llm_agent_core::{CoreConfig, Event, SessionConfig};
use cosmos_llm_tool::{Completer, ParameterType, Registry, ToolDefinition};
use serde_json::json;
use tokio::sync::mpsc::Receiver;

/// A completer that remembers the requests it saw.
pub struct Recording {
    inner: Client,
    seen: std::sync::Mutex<Vec<cosmos_llm::CompletionRequest>>,
}

impl Recording {
    pub fn requests(&self) -> Vec<cosmos_llm::CompletionRequest> {
        self.seen.lock().unwrap().clone()
    }
    pub fn last_request(&self) -> Option<cosmos_llm::CompletionRequest> {
        self.requests().pop()
    }
}

impl Completer for Recording {
    fn completion<'a>(
        &'a self,
        req: &'a cosmos_llm::CompletionRequest,
    ) -> futures_util::future::BoxFuture<
        'a,
        Result<cosmos_llm::CompletionResponse, cosmos_llm::CosmosError>,
    > {
        self.seen.lock().unwrap().push(req.clone());
        Box::pin(async move { Completer::completion(&self.inner, req).await })
    }
}

pub fn completer(mock: MockProvider) -> Arc<Recording> {
    Arc::new(Recording {
        inner: Client::from_provider(Box::new(mock)),
        seen: Default::default(),
    })
}

pub fn config(cwd: &std::path::Path, sessions: &std::path::Path) -> CoreConfig {
    let mut c = CoreConfig::default();
    c.model = Some("mock-model".into());
    c.cwd = Some(cwd.to_owned());
    c.instructions = false;
    let mut s = SessionConfig::default();
    s.dir = Some(sessions.to_owned());
    c.sessions = s;
    c
}

pub fn registry() -> Registry {
    let mut r = Registry::new();
    r.register(
        ToolDefinition::new("echo")
            .description("echoes")
            .param("text", ParameterType::String, true, "text")
            .handler(|p| Ok(json!(p["text"].as_str().unwrap_or("")))),
    );
    r.register(
        ToolDefinition::new("peek")
            .description("reads")
            .read_only(true)
            .handler(|_| Ok(json!("peeked"))),
    );
    r.register(
        ToolDefinition::new("slow")
            .description("waits")
            .context_handler(|_p, ctx| {
                Box::pin(async move {
                    tokio::select! {
                        _ = ctx.cancel.cancelled() => Err("cancelled".to_owned()),
                        _ = tokio::time::sleep(Duration::from_secs(30)) => Ok(json!("late")),
                    }
                })
            }),
    );
    r
}

/// Receives events until `pred` matches one, returning everything seen.
pub async fn until(rx: &mut Receiver<Event>, pred: impl Fn(&Event) -> bool) -> Vec<Event> {
    let mut seen = Vec::new();
    loop {
        let ev = tokio::time::timeout(Duration::from_secs(10), rx.recv())
            .await
            .expect("timed out waiting for an event")
            .expect("event channel closed");
        let done = pred(&ev);
        seen.push(ev);
        if done {
            return seen;
        }
    }
}

pub fn run_finished(e: &Event) -> bool {
    matches!(e, Event::RunFinished { .. })
}
