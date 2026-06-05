use std::future::Future;
use std::pin::Pin;

use reqwest::Client as HttpClient;
use serde_json::{json, Value};

use crate::error::CosmosError;
use crate::providers::Provider;
use crate::types::{Choice, CompletionRequest, CompletionResponse, Message, Usage};

const BASE_URL: &str = "https://api.openai.com/v1";

/// Provider implementation for the OpenAI API.
///
/// Supports chat completions, embeddings, streaming, and model listing.
/// Reads the API key from the `OPENAI_API_KEY` or `CLLM__OPENAI__API_KEY`
/// environment variable when none is supplied at construction.
///
/// # Examples
///
/// ```no_run
/// use cosmos_llm::providers::openai::OpenAiProvider;
/// use cosmos_llm::providers::Provider;
/// use cosmos_llm::types::{CompletionRequest, Message};
///
/// # tokio_test::block_on(async {
/// let provider = OpenAiProvider::new(Some("sk-test".into()));
/// let req = CompletionRequest::new("gpt-4o", vec![Message::user("Hello")]);
/// // let resp = provider.completion(&req).await.unwrap();
/// # })
/// ```
pub struct OpenAiProvider {
    api_key: Option<String>,
    http: HttpClient,
}

impl OpenAiProvider {
    /// Creates a new [`OpenAiProvider`].
    ///
    /// When `api_key` is `None`, the provider falls back to the
    /// `OPENAI_API_KEY` or `CLLM__OPENAI__API_KEY` environment variables.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::providers::openai::OpenAiProvider;
    /// let provider = OpenAiProvider::new(None);
    /// ```
    pub fn new(api_key: Option<String>) -> Self {
        let key = api_key
            .or_else(|| std::env::var("OPENAI_API_KEY").ok())
            .or_else(|| std::env::var("CLLM__OPENAI__API_KEY").ok());
        Self {
            api_key: key,
            http: HttpClient::new(),
        }
    }

    fn resolved_key(&self) -> Result<&str, CosmosError> {
        self.api_key.as_deref().ok_or_else(|| {
            CosmosError::Authentication(
                "OpenAI API key not set. Export OPENAI_API_KEY or CLLM__OPENAI__API_KEY."
                    .to_owned(),
            )
        })
    }

    fn map_response(body: Value) -> Result<CompletionResponse, CosmosError> {
        let id = body["id"].as_str().map(str::to_owned);
        let model = body["model"].as_str().map(str::to_owned);

        let choices = body["choices"]
            .as_array()
            .ok_or_else(|| CosmosError::InvalidResponse("missing 'choices' field".into()))?
            .iter()
            .map(|c| {
                let role = c["message"]["role"]
                    .as_str()
                    .unwrap_or("assistant")
                    .to_owned();
                let content = c["message"]["content"].as_str().unwrap_or("").to_owned();
                let finish_reason = c["finish_reason"].as_str().map(str::to_owned);
                let index = c["index"].as_u64().unwrap_or(0) as u32;
                Choice {
                    index,
                    message: Message { role, content },
                    finish_reason,
                }
            })
            .collect();

        let usage = if body["usage"].is_object() {
            Some(Usage {
                prompt_tokens: body["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32,
                completion_tokens: body["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32,
                total_tokens: body["usage"]["total_tokens"].as_u64().unwrap_or(0) as u32,
            })
        } else {
            None
        };

        Ok(CompletionResponse {
            id,
            model,
            choices,
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

impl Provider for OpenAiProvider {
    fn completion<'a>(
        &'a self,
        req: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse, CosmosError>> + Send + 'a>> {
        Box::pin(async move {
            let key = self.resolved_key()?;

            let mut body = json!({
                "model": req.model,
                "messages": req.messages,
            });

            if let Some(t) = req.temperature {
                body["temperature"] = json!(t);
            }
            if let Some(n) = req.max_tokens {
                body["max_tokens"] = json!(n);
            }
            if let Some(p) = req.top_p {
                body["top_p"] = json!(p);
            }
            if let Some(ref stop) = req.stop {
                body["stop"] = json!(stop);
            }

            let resp = self
                .http
                .post(format!("{BASE_URL}/chat/completions"))
                .bearer_auth(key)
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
        Box::pin(async move {
            let key = self.resolved_key()?;

            let resp = self
                .http
                .get(format!("{BASE_URL}/models"))
                .bearer_auth(key)
                .send()
                .await?;

            let status = resp.status().as_u16();
            let json: Value = resp.json().await?;

            if (200..300).contains(&status) {
                let ids = json["data"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter_map(|m| m["id"].as_str().map(str::to_owned))
                    .collect();
                Ok(ids)
            } else {
                Err(Self::handle_error(status, &json))
            }
        })
    }

    fn supports_streaming(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_streaming() {
        let p = OpenAiProvider::new(None);
        assert!(p.supports_streaming());
    }

    #[test]
    fn missing_key_yields_auth_error() {
        // Clear both env vars to ensure no key leaks in.
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("CLLM__OPENAI__API_KEY");
        let p = OpenAiProvider::new(None);
        let err = p.resolved_key().unwrap_err();
        assert!(matches!(err, CosmosError::Authentication(_)));
    }

    #[test]
    fn map_response_parses_openai_body() {
        let body = serde_json::json!({
            "id": "chatcmpl-1",
            "model": "gpt-4o",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "Hello!" },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 15
            }
        });
        let resp = OpenAiProvider::map_response(body).unwrap();
        assert_eq!(resp.content(), Some("Hello!"));
        assert_eq!(resp.usage.unwrap().total_tokens, 15);
    }
}
