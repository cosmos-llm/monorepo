#![cfg(feature = "mock")]

//! Client-level behaviour: cancellation, model metadata, reasoning checks.

use std::sync::Arc;
use std::time::Duration;

use cosmos_llm::providers::mock::MockProvider;
use cosmos_llm::providers::Provider;
use cosmos_llm::{
    CancellationToken, Client, CompletionRequest, CosmosError, Message, ModelInfo, Pricing,
    Reasoning, ReasoningEffort,
};
use futures_util::StreamExt;
use retry_policy::RetryPolicy;

fn req(token: &CancellationToken) -> CompletionRequest {
    CompletionRequest::new("mock-model", vec![Message::user("hi")]).with_cancel(token.clone())
}

#[tokio::test(start_paused = true)]
async fn cancelling_aborts_a_slow_completion_promptly() {
    let provider = MockProvider::new()
        .with_delay(Duration::from_secs(60))
        .respond("never");
    let client = Client::from_provider(Box::new(provider));
    let token = CancellationToken::new();

    let canceller = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        canceller.cancel();
    });

    let started = tokio::time::Instant::now();
    let err = client.completion(req(&token)).await.unwrap_err();
    assert!(err.is_cancelled());
    assert!(!err.is_retryable());
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[tokio::test]
async fn an_already_cancelled_token_fails_without_calling_the_provider() {
    let provider = Arc::new(MockProvider::new().respond("x"));
    let client = Client::from_provider(Box::new(SharedMock(provider.clone())));
    let token = CancellationToken::new();
    token.cancel();

    let err = client.completion(req(&token)).await.unwrap_err();
    assert!(matches!(err, CosmosError::Cancelled));
    assert_eq!(provider.requests().len(), 0);
}

#[tokio::test(start_paused = true)]
async fn a_cancel_during_backoff_starts_no_second_attempt() {
    let provider = Arc::new(
        MockProvider::new()
            .fail_times(1, || CosmosError::Server {
                provider: "mock".into(),
                status: 503,
                message: "overloaded".into(),
            })
            .respond("late"),
    );
    let client = Client::from_provider(Box::new(SharedMock(provider.clone()))).with_retry(
        RetryPolicy::new()
            .max_attempts(3)
            .initial_backoff(Duration::from_secs(30)),
    );
    let token = CancellationToken::new();

    let canceller = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        canceller.cancel();
    });

    let err = client.completion(req(&token)).await.unwrap_err();
    assert!(err.is_cancelled());
    assert_eq!(
        provider.requests().len(),
        1,
        "no second attempt after cancel"
    );
}

#[tokio::test]
async fn a_cancelled_stream_yields_cancelled_once_then_ends() {
    let provider = MockProvider::new().respond("one two three four");
    let client = Client::from_provider(Box::new(provider));
    let token = CancellationToken::new();

    let mut stream = client.stream_completion(req(&token)).await.unwrap();
    assert!(stream.next().await.unwrap().is_ok());
    assert!(stream.next().await.unwrap().is_ok());

    token.cancel();
    let item = stream.next().await.unwrap();
    assert!(matches!(item, Err(CosmosError::Cancelled)));
    assert!(stream.next().await.is_none());
}

#[tokio::test]
async fn an_uncancelled_token_changes_nothing() {
    let client = Client::from_provider(Box::new(MockProvider::new().respond("ok")));
    let token = CancellationToken::new();
    let resp = client.completion(req(&token)).await.unwrap();
    assert_eq!(resp.text(), "ok");
}

#[test]
fn reasoning_on_a_model_known_not_to_support_it_fails_locally() {
    let client = Client::from_provider(Box::new(MockProvider::new().respond("x")));
    let request = CompletionRequest::new("gpt-4o", vec![Message::user("hi")])
        .with_reasoning(Reasoning::effort(ReasoningEffort::High));
    let err = tokio_test::block_on(client.completion(request)).unwrap_err();
    assert!(matches!(err, CosmosError::Configuration { .. }), "{err}");
}

#[test]
fn reasoning_on_an_unknown_model_is_sent_through() {
    let client = Client::from_provider(Box::new(MockProvider::new().respond("x")));
    let request = CompletionRequest::new("mystery-model", vec![Message::user("hi")])
        .with_reasoning(Reasoning::effort(ReasoningEffort::High));
    assert!(tokio_test::block_on(client.completion(request)).is_ok());
}

#[tokio::test]
async fn model_info_prefers_an_override_over_the_table() {
    let client = Client::from_provider(Box::new(MockProvider::new())).with_model_info(
        ModelInfo::new("mock", "gpt-4o")
            .with_context_window(1234)
            .with_pricing(Pricing::new(1.0, 2.0)),
    );
    let info = client.model_info("gpt-4o").await.unwrap();
    assert_eq!(info.context_window, Some(1234));
}

#[tokio::test]
async fn model_info_falls_back_to_the_table_and_then_to_unknown() {
    let client = Client::from_provider(Box::new(MockProvider::new()));
    let known = client
        .model_info("claude-3-5-sonnet-20241022")
        .await
        .unwrap();
    assert_eq!(known.context_window, Some(200_000));
    assert_eq!(known.id, "claude-3-5-sonnet-20241022");

    let unknown = client.model_info("home-grown").await.unwrap();
    assert_eq!(unknown.context_window, None);
    assert!(!unknown.supports_tools);
}

#[tokio::test]
async fn models_info_lists_the_provider_models_with_table_data() {
    let provider = MockProvider::new().with_models(vec!["gpt-4o".into(), "home-grown".into()]);
    let client = Client::from_provider(Box::new(provider))
        .with_model_info(ModelInfo::new("mock", "extra").with_context_window(8));
    let infos = client.models_info().await.unwrap();
    assert_eq!(infos.len(), 3);
    assert_eq!(infos[0].context_window, Some(128_000));
    assert_eq!(infos[1].context_window, None);
    assert_eq!(infos[2].id, "extra");
}

/// Lets a test keep a handle on the mock while the client owns the provider.
struct SharedMock(Arc<MockProvider>);

impl Provider for SharedMock {
    fn completion<'a>(
        &'a self,
        req: &'a CompletionRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<cosmos_llm::CompletionResponse, CosmosError>>
                + Send
                + 'a,
        >,
    > {
        self.0.completion(req)
    }

    fn models<'a>(
        &'a self,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<String>, CosmosError>> + Send + 'a>,
    > {
        self.0.models()
    }

    fn name(&self) -> &str {
        "mock"
    }
}
