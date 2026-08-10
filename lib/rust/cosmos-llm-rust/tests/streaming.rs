//! End-to-end streaming tests against a mock HTTP server.
//!
//! These exercise the whole path — request body, SSE framing over a real
//! socket, per-provider chunk parsing, and accumulation — rather than any one
//! layer in isolation. `Client::new_with_base_url` points each provider at the
//! mock server.

use cosmos_llm::{Client, CompletionRequest, CosmosError, Message, StreamAccumulator};
use futures_util::StreamExt;

/// Collects a whole stream into an accumulator, failing on the first error.
async fn drain(stream: cosmos_llm::CompletionStream) -> Result<StreamAccumulator, CosmosError> {
    let mut stream = stream;
    let mut acc = StreamAccumulator::new();
    while let Some(chunk) = stream.next().await {
        acc.push(&chunk?);
    }
    Ok(acc)
}

// ── OpenAI ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn openai_streams_text_over_http() {
    let mut server = mockito::Server::new_async().await;

    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"\"}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"The capital \"}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"is Paris.\"}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":4,\"total_tokens\":11}}\n\n",
        "data: [DONE]\n\n",
    );

    let mock = server
        .mock("POST", "/chat/completions")
        .match_header("authorization", "Bearer sk-test")
        // The request must actually ask for a stream, or the server would
        // reply with a single JSON body instead.
        .match_body(mockito::Matcher::PartialJson(serde_json::json!({
            "stream": true,
            "stream_options": { "include_usage": true }
        })))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let client = Client::new_with_base_url("openai", "sk-test", server.url())
        .unwrap()
        .with_model("gpt-4o");

    let acc = drain(
        client
            .stream("What is the capital of France?")
            .await
            .unwrap(),
    )
    .await
    .unwrap();

    mock.assert_async().await;

    let resp = acc.into_response();
    assert_eq!(resp.text(), "The capital is Paris.");
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
    assert_eq!(resp.usage.unwrap().total_tokens, 11);
}

#[tokio::test]
async fn openai_streams_tool_calls_over_http() {
    let mut server = mockito::Server::new_async().await;

    // Arguments arrive as several JSON fragments, exactly as OpenAI sends
    // them: no single chunk contains parseable JSON.
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"ci\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"ty\\\":\\\"Bos\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"ton\\\"}\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n",
    );

    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let client = Client::new_with_base_url("openai", "sk-test", server.url()).unwrap();
    let req = CompletionRequest::new("gpt-4o", vec![Message::user("Weather in Boston?")]);

    let acc = drain(client.stream_completion(req).await.unwrap())
        .await
        .unwrap();

    mock.assert_async().await;

    let resp = acc.into_response();
    assert!(resp.tool_use());
    let calls = resp.tool_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].id, "call_1");
    assert_eq!(calls[0].name, "get_weather");
    assert_eq!(calls[0].input, serde_json::json!({"city": "Boston"}));
}

#[tokio::test]
async fn openai_stream_rejects_error_status_before_streaming() {
    let mut server = mockito::Server::new_async().await;

    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(401)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error":{"message":"Invalid API key"}}"#)
        .create_async()
        .await;

    let client = Client::new_with_base_url("openai", "sk-bad", server.url())
        .unwrap()
        .with_model("gpt-4o");

    assert!(matches!(
        client.stream("hi").await,
        Err(CosmosError::Authentication { .. })
    ));

    mock.assert_async().await;
}

#[tokio::test]
async fn openai_stream_surfaces_mid_stream_error() {
    let mut server = mockito::Server::new_async().await;

    // A 200 commits the connection to streaming; the failure arrives as a
    // payload, so it has to surface as a stream item rather than a call error.
    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"}}]}\n\n",
        "data: {\"error\":{\"message\":\"upstream exploded\"}}\n\n",
    );

    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let client = Client::new_with_base_url("openai", "sk-test", server.url())
        .unwrap()
        .with_model("gpt-4o");

    let mut stream = client.stream("hi").await.unwrap();

    let first = stream.next().await.unwrap().unwrap();
    assert_eq!(first.delta, "partial");

    // A mid-stream failure is classified the same way a pre-stream one is, so
    // a caller can tell a retryable overload from a permanent rejection. With
    // no code in the payload, a server-side failure is the safe reading.
    let err = stream.next().await.unwrap().unwrap_err();
    assert!(matches!(
        err,
        CosmosError::Server { ref message, .. } if message.contains("upstream exploded")
    ));
    assert!(err.is_retryable());

    // The stream terminates after yielding its error.
    assert!(stream.next().await.is_none());
}

#[tokio::test]
async fn stream_to_completion_reports_every_chunk() {
    let mut server = mockito::Server::new_async().await;

    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"one \"}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"two \"}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"three\"}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n",
    );

    server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let client = Client::new_with_base_url("openai", "sk-test", server.url()).unwrap();
    let req = CompletionRequest::new("gpt-4o", vec![Message::user("count")]);

    let mut seen = Vec::new();
    let resp = client
        .stream_to_completion(req, |chunk| {
            if !chunk.delta.is_empty() {
                seen.push(chunk.delta.clone());
            }
        })
        .await
        .unwrap();

    assert_eq!(seen, vec!["one ", "two ", "three"]);
    assert_eq!(resp.text(), "one two three");
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("stop"));
}

// ── Anthropic ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn anthropic_streams_text_and_usage_over_http() {
    let mut server = mockito::Server::new_async().await;

    let body = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"usage\":{\"input_tokens\":10}}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: ping\n",
        "data: {\"type\":\"ping\"}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\", world\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":5}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );

    let mock = server
        .mock("POST", "/messages")
        .match_header("x-api-key", "sk-ant-test")
        .match_header("anthropic-version", "2023-06-01")
        .match_body(mockito::Matcher::PartialJson(
            serde_json::json!({ "stream": true }),
        ))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let client = Client::new_with_base_url("anthropic", "sk-ant-test", server.url())
        .unwrap()
        .with_model("claude-3-5-sonnet-20241022");

    let acc = drain(client.stream("Say hello").await.unwrap())
        .await
        .unwrap();

    mock.assert_async().await;

    let resp = acc.into_response();
    assert_eq!(resp.text(), "Hello, world");
    assert_eq!(resp.choices[0].finish_reason.as_deref(), Some("end_turn"));

    // input_tokens come from message_start, output_tokens from message_delta.
    let usage = resp.usage.unwrap();
    assert_eq!(usage.prompt_tokens, 10);
    assert_eq!(usage.completion_tokens, 5);
    assert_eq!(usage.total_tokens, 15);
}

#[tokio::test]
async fn anthropic_streams_tool_call_after_text_block() {
    let mut server = mockito::Server::new_async().await;

    // Block 0 is text and block 1 is the tool call, so the tool's *block*
    // index is 1 while its *tool-call* index must be 0.
    let body = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_2\",\"usage\":{\"input_tokens\":12}}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Let me check.\"}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"get_weather\",\"input\":{}}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"city\\\":\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"Boston\\\"}\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":9}}\n\n",
    );

    server
        .mock("POST", "/messages")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let client = Client::new_with_base_url("anthropic", "sk-ant-test", server.url()).unwrap();
    let req = CompletionRequest::new(
        "claude-3-5-sonnet-20241022",
        vec![Message::user("Weather in Boston?")],
    );

    let acc = drain(client.stream_completion(req).await.unwrap())
        .await
        .unwrap();

    let resp = acc.into_response();
    assert_eq!(resp.text(), "Let me check.");
    assert!(resp.tool_use());

    let calls = resp.tool_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].id, "toolu_1");
    assert_eq!(calls[0].name, "get_weather");
    assert_eq!(calls[0].input, serde_json::json!({"city": "Boston"}));
}

#[tokio::test]
async fn anthropic_stream_surfaces_error_event() {
    let mut server = mockito::Server::new_async().await;

    let body = concat!(
        "event: error\n",
        "data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n",
    );

    server
        .mock("POST", "/messages")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let client = Client::new_with_base_url("anthropic", "sk-ant-test", server.url())
        .unwrap()
        .with_model("claude-3-5-sonnet-20241022");

    let mut stream = client.stream("hi").await.unwrap();
    let err = stream.next().await.unwrap().unwrap_err();

    // Anthropic's most common streaming failure. It has to come back retryable
    // — that is the whole reason for classifying mid-stream errors rather than
    // reporting them all as generic streaming failures.
    assert!(matches!(
        err,
        CosmosError::Server { ref message, .. } if message == "Overloaded"
    ));
    assert!(err.is_retryable());
    assert_eq!(err.provider(), "anthropic");
}

// ── OpenRouter ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn openrouter_streams_with_attribution_headers() {
    let mut server = mockito::Server::new_async().await;

    let body = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"routed\"}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n",
    );

    let mock = server
        .mock("POST", "/chat/completions")
        .match_header("authorization", "Bearer sk-or-test")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(body)
        .create_async()
        .await;

    let client = Client::new_with_base_url("openrouter", "sk-or-test", server.url())
        .unwrap()
        .with_model("anthropic/claude-3.5-sonnet");

    let acc = drain(client.stream("hi").await.unwrap()).await.unwrap();

    mock.assert_async().await;
    assert_eq!(acc.text(), "routed");
    assert_eq!(acc.finish_reason(), Some("stop"));
}

// ── Non-streaming, via the same override ──────────────────────────────────────

#[tokio::test]
async fn base_url_override_applies_to_non_streaming_calls_too() {
    let mut server = mockito::Server::new_async().await;

    let mock = server
        .mock("POST", "/chat/completions")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"id":"chatcmpl-1","model":"gpt-4o","choices":[{"index":0,
                "message":{"role":"assistant","content":"Paris"},"finish_reason":"stop"}],
                "usage":{"prompt_tokens":5,"completion_tokens":1,"total_tokens":6}}"#,
        )
        .create_async()
        .await;

    let client = Client::new_with_base_url("openai", "sk-test", server.url())
        .unwrap()
        .with_model("gpt-4o");

    let text = client.complete("Capital of France?").await.unwrap();

    mock.assert_async().await;
    assert_eq!(text, "Paris");
}

#[tokio::test]
async fn models_endpoint_honours_base_url_override() {
    let mut server = mockito::Server::new_async().await;

    let mock = server
        .mock("GET", "/models")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"data":[{"id":"gpt-4o"},{"id":"gpt-4o-mini"}]}"#)
        .create_async()
        .await;

    let client = Client::new_with_base_url("openai", "sk-test", server.url()).unwrap();
    let models = client.models().await.unwrap();

    mock.assert_async().await;
    assert_eq!(models, vec!["gpt-4o", "gpt-4o-mini"]);
}

#[test]
fn base_url_trailing_slash_is_trimmed() {
    use cosmos_llm::providers::openai::OpenAiProvider;

    let p = OpenAiProvider::new(Some("sk-test".into())).with_base_url("http://example.com/v1/");
    assert_eq!(p.base_url(), "http://example.com/v1");
}

/// The env-var fallback and the builder both feed the same field, so this
/// checks the env path and that an explicit `with_base_url` still wins.
///
/// Kept in one test because env vars are process-global: splitting it would
/// let the two halves race under the default parallel test runner.
#[test]
fn base_url_env_override_is_honoured_and_overridable() {
    use cosmos_llm::providers::anthropic::{AnthropicProvider, DEFAULT_BASE_URL};

    std::env::remove_var("ANTHROPIC_BASE_URL");
    std::env::remove_var("CLLM__ANTHROPIC__BASE_URL");
    assert_eq!(
        AnthropicProvider::new(Some("k".into())).base_url(),
        DEFAULT_BASE_URL
    );

    std::env::set_var("ANTHROPIC_BASE_URL", "http://from-env:9000/v1");
    assert_eq!(
        AnthropicProvider::new(Some("k".into())).base_url(),
        "http://from-env:9000/v1"
    );

    // An explicit builder call takes precedence over the environment.
    assert_eq!(
        AnthropicProvider::new(Some("k".into()))
            .with_base_url("http://explicit:1234/v1")
            .base_url(),
        "http://explicit:1234/v1"
    );

    std::env::remove_var("ANTHROPIC_BASE_URL");
}
