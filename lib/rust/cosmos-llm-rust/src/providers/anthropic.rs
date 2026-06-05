use std::future::Future;
use std::pin::Pin;

use reqwest::Client as HttpClient;
use serde_json::{json, Value};

use crate::error::CosmosError;
use crate::providers::Provider;
use crate::types::{Choice, CompletionRequest, CompletionResponse, Message, Usage};

const BASE_URL: &str = "https://api.anthropic.com/v1";
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Static list of known Claude models, returned by [`AnthropicProvider::models`].
///
/// Anthropic's models endpoint requires a paid account; this list is used as
/// a fallback so callers can enumerate models without an active subscription.
const KNOWN_MODELS: &[&str] = &[
    "claude-opus-4-8",
    "claude-sonnet-4-6",
    "claude-haiku-4-5-20251001",
    "claude-3-5-sonnet-20241022",
    "claude-3-5-haiku-20241022",
    "claude-3-opus-20240229",
    "claude-3-haiku-20240307",
];

/// Provider implementation for the Anthropic Messages API.
///
/// Supports chat completions (with optional system message) and model listing.
/// Reads the API key from the `ANTHROPIC_API_KEY` or
/// `CLLM__ANTHROPIC__API_KEY` environment variable when none is supplied at
/// construction.
///
/// # Examples
///
/// ```no_run
/// use cosmos_llm::providers::anthropic::AnthropicProvider;
/// use cosmos_llm::providers::Provider;
/// use cosmos_llm::types::{CompletionRequest, Message};
///
/// # tokio_test::block_on(async {
/// let provider = AnthropicProvider::new(Some("sk-ant-test".into()));
/// let req = CompletionRequest::new(
///     "claude-3-5-sonnet-20241022",
///     vec![Message::user("Hello!")],
/// );
/// // let resp = provider.completion(&req).await.unwrap();
/// # })
/// ```
pub struct AnthropicProvider {
    api_key: Option<String>,
    http: HttpClient,
}

impl AnthropicProvider {
    /// Creates a new [`AnthropicProvider`].
    ///
    /// When `api_key` is `None`, falls back to `ANTHROPIC_API_KEY` or
    /// `CLLM__ANTHROPIC__API_KEY` environment variables.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::anthropic::AnthropicProvider;
    /// let provider = AnthropicProvider::new(None);
    /// ```
    pub fn new(api_key: Option<String>) -> Self {
        let key = api_key
            .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
            .or_else(|| std::env::var("CLLM__ANTHROPIC__API_KEY").ok());
        Self {
            api_key: key,
            http: HttpClient::new(),
        }
    }

    fn resolved_key(&self) -> Result<&str, CosmosError> {
        self.api_key.as_deref().ok_or_else(|| {
            CosmosError::Authentication(
                "Anthropic API key not set. Export ANTHROPIC_API_KEY or CLLM__ANTHROPIC__API_KEY."
                    .to_owned(),
            )
        })
    }

    /// Splits messages into an optional system string and the remaining chat messages.
    ///
    /// Anthropic's API separates the system prompt from the message array.
    fn split_system(messages: &[Message]) -> (Option<String>, Vec<&Message>) {
        let mut system = None;
        let mut chat = Vec::with_capacity(messages.len());
        for msg in messages {
            if msg.role == "system" {
                system = Some(msg.content.clone());
            } else {
                chat.push(msg);
            }
        }
        (system, chat)
    }

    fn map_response(body: Value) -> Result<CompletionResponse, CosmosError> {
        let id = body["id"].as_str().map(str::to_owned);
        let model = body["model"].as_str().map(str::to_owned);

        // Anthropic returns `content` as an array of blocks.
        let text = body["content"]
            .as_array()
            .and_then(|blocks| {
                blocks
                    .iter()
                    .filter(|b| b["type"] == "text")
                    .map(|b| b["text"].as_str().unwrap_or("").to_owned())
                    .reduce(|a, b| a + &b)
            })
            .unwrap_or_default();

        let finish_reason = body["stop_reason"].as_str().map(str::to_owned);

        let usage = if body["usage"].is_object() {
            Some(Usage {
                prompt_tokens: body["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32,
                completion_tokens: body["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32,
                total_tokens: (body["usage"]["input_tokens"].as_u64().unwrap_or(0)
                    + body["usage"]["output_tokens"].as_u64().unwrap_or(0))
                    as u32,
            })
        } else {
            None
        };

        Ok(CompletionResponse {
            id,
            model,
            choices: vec![Choice {
                index: 0,
                message: Message::assistant(text),
                finish_reason,
            }],
            usage,
        })
    }

    fn handle_error(status: u16, body: &Value) -> CosmosError {
        let msg = body["error"]["message"]
            .as_str()
            .unwrap_or("unknown error")
            .to_owned();
        match status {
            401 => CosmosError::Authentication(msg),
            429 => CosmosError::RateLimit(msg),
            400 | 404 => CosmosError::InvalidRequest(msg),
            s if s >= 500 => CosmosError::Server(msg),
            _ => CosmosError::InvalidResponse(format!("unexpected status {status}: {msg}")),
        }
    }
}

impl Provider for AnthropicProvider {
    fn completion<'a>(
        &'a self,
        req: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse, CosmosError>> + Send + 'a>> {
        Box::pin(async move {
            let key = self.resolved_key()?;
            let (system, chat_msgs) = Self::split_system(&req.messages);

            let messages: Vec<Value> = chat_msgs
                .iter()
                .map(|m| json!({ "role": m.role, "content": m.content }))
                .collect();

            let mut body = json!({
                "model": req.model,
                "messages": messages,
                "max_tokens": req.max_tokens.unwrap_or(1024),
            });

            if let Some(ref sys) = system {
                body["system"] = json!(sys);
            }
            if let Some(t) = req.temperature {
                body["temperature"] = json!(t);
            }
            if let Some(p) = req.top_p {
                body["top_p"] = json!(p);
            }
            if let Some(ref stop) = req.stop {
                body["stop_sequences"] = json!(stop);
            }

            let resp = self
                .http
                .post(format!("{BASE_URL}/messages"))
                .header("x-api-key", key)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .json(&body)
                .send()
                .await?;

            let status = resp.status().as_u16();
            let json: Value = resp.json().await?;

            if (200..300).contains(&status) {
                Self::map_response(json)
            } else {
                Err(Self::handle_error(status, &json))
            }
        })
    }

    fn models<'a>(
        &'a self,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>, CosmosError>> + Send + 'a>> {
        Box::pin(async move { Ok(KNOWN_MODELS.iter().map(|s| s.to_string()).collect()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_system_separates_messages() {
        let msgs = vec![
            Message::system("be helpful"),
            Message::user("hi"),
            Message::assistant("hello"),
        ];
        let (sys, chat) = AnthropicProvider::split_system(&msgs);
        assert_eq!(sys, Some("be helpful".into()));
        assert_eq!(chat.len(), 2);
    }

    #[test]
    fn split_system_no_system() {
        let msgs = vec![Message::user("hi")];
        let (sys, chat) = AnthropicProvider::split_system(&msgs);
        assert!(sys.is_none());
        assert_eq!(chat.len(), 1);
    }

    #[test]
    fn missing_key_yields_auth_error() {
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("CLLM__ANTHROPIC__API_KEY");
        let p = AnthropicProvider::new(None);
        assert!(matches!(
            p.resolved_key(),
            Err(CosmosError::Authentication(_))
        ));
    }

    #[test]
    fn map_response_parses_anthropic_body() {
        let body = serde_json::json!({
            "id": "msg_1",
            "model": "claude-3-5-sonnet-20241022",
            "content": [{ "type": "text", "text": "Hi there!" }],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 8, "output_tokens": 4 }
        });
        let resp = AnthropicProvider::map_response(body).unwrap();
        assert_eq!(resp.content(), Some("Hi there!"));
        assert_eq!(resp.usage.unwrap().total_tokens, 12);
    }

    #[tokio::test]
    async fn models_returns_known_list() {
        let p = AnthropicProvider::new(Some("key".into()));
        let list = p.models().await.unwrap();
        assert!(list.contains(&"claude-sonnet-4-6".to_string()));
    }
}
