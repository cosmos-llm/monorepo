//! `Client::with_retry` against the mock provider.
//!
//! This is the pairing the plan's 030.010 verification list describes: the mock
//! returns a scripted error class, and the retry policy decides what to do about
//! it. No network, no clock.

#![cfg(feature = "mock")]

use std::time::Duration;

use cosmos_llm::providers::mock::MockProvider;
use cosmos_llm::{Client, CompletionRequest, CosmosError, Message};
use retry_policy::{Jitter, RetryPolicy};

/// A policy that retries promptly, so tests do not wait on a real clock.
fn quick(max_attempts: u32) -> RetryPolicy {
    RetryPolicy::new()
        .max_attempts(max_attempts)
        .initial_backoff(Duration::from_micros(1))
        .jitter(Jitter::None)
}

fn request() -> CompletionRequest {
    CompletionRequest::new("mock-model", vec![Message::user("hi")])
}

#[tokio::test]
async fn retry_is_off_unless_asked_for() {
    // The default matters: a library that retried on its own would double up
    // with a caller doing the same, and spend their money doing it.
    let provider = MockProvider::new().fail_times(3, || CosmosError::Server {
        provider: "mock".into(),
        status: 503,
        message: "overloaded".into(),
    });

    let client = Client::from_provider(Box::new(provider));
    assert!(client.retry_policy().is_none());

    let err = client.completion(request()).await.unwrap_err();
    assert!(matches!(err, CosmosError::Server { .. }));
}

#[tokio::test]
async fn a_transient_failure_is_retried_until_it_succeeds() {
    let provider = MockProvider::new()
        .fail_times(2, || CosmosError::Server {
            provider: "mock".into(),
            status: 503,
            message: "overloaded".into(),
        })
        .respond("recovered");

    let client = Client::from_provider(Box::new(provider)).with_retry(quick(4));

    let resp = client.completion(request()).await.unwrap();
    assert_eq!(resp.text(), "recovered");
}

#[tokio::test]
async fn a_non_retryable_failure_costs_exactly_one_attempt() {
    // Scripted with three failures but only one should be consumed: a bad key
    // will still be a bad key on the second attempt.
    let provider = MockProvider::new().fail_times(3, || CosmosError::Authentication {
        provider: "mock".into(),
        message: "invalid api key".into(),
    });

    let client = Client::from_provider(Box::new(provider)).with_retry(quick(3));

    let err = client.completion(request()).await.unwrap_err();
    assert!(matches!(err, CosmosError::Authentication { .. }));
}

#[tokio::test]
async fn an_exhausted_policy_surfaces_the_underlying_error() {
    // The signature stays `CosmosError`, so a caller that never opted into
    // retrying does not have to learn a new error type.
    let provider = MockProvider::new().fail_times(5, || CosmosError::Server {
        provider: "mock".into(),
        status: 500,
        message: "still broken".into(),
    });

    let client = Client::from_provider(Box::new(provider)).with_retry(quick(3));

    let err = client.completion(request()).await.unwrap_err();
    assert!(matches!(err, CosmosError::Server { ref message, .. } if message == "still broken"));
}

#[tokio::test]
async fn quota_exhaustion_is_not_retried_even_though_it_arrives_as_429() {
    // The distinction the shared error mapper exists for. A throttling 429
    // clears on its own; an empty account does not, and retrying it just burns
    // attempts.
    let provider = MockProvider::new()
        .fail_times(3, || CosmosError::InsufficientQuota {
            provider: "mock".into(),
            message: "add credits".into(),
        })
        .respond("never reached");

    let client = Client::from_provider(Box::new(provider)).with_retry(quick(4));

    let err = client.completion(request()).await.unwrap_err();
    assert!(matches!(err, CosmosError::InsufficientQuota { .. }));
}

#[tokio::test]
async fn a_rate_limit_retry_after_is_honored() {
    let provider = MockProvider::new()
        .fail_times(1, || CosmosError::RateLimit {
            provider: "mock".into(),
            message: "slow down".into(),
            // Small enough to keep the test fast, and well under the policy cap.
            retry_after: Some(Duration::from_millis(1)),
        })
        .respond("after waiting");

    let client = Client::from_provider(Box::new(provider))
        .with_retry(quick(3).max_retry_after(Duration::from_secs(30)));

    let resp = client.completion(request()).await.unwrap();
    assert_eq!(resp.text(), "after waiting");
}

#[tokio::test]
async fn an_absurd_retry_after_fails_rather_than_hanging() {
    // Retry-After: 3600 is an instruction to hang for an hour. The cap is what
    // stops a client from obeying it.
    let provider = MockProvider::new()
        .fail_times(1, || CosmosError::RateLimit {
            provider: "mock".into(),
            message: "come back later".into(),
            retry_after: Some(Duration::from_secs(3600)),
        })
        .respond("never reached");

    let client = Client::from_provider(Box::new(provider))
        .with_retry(quick(3).max_retry_after(Duration::from_secs(30)));

    let err = client.completion(request()).await.unwrap_err();
    assert!(matches!(err, CosmosError::RateLimit { .. }));
}

#[tokio::test]
async fn retry_applies_to_the_complete_helper_too() {
    // `complete` used to call the provider directly, which would have silently
    // skipped the policy.
    let provider = MockProvider::new()
        .fail_times(1, || CosmosError::Server {
            provider: "mock".into(),
            status: 503,
            message: "overloaded".into(),
        })
        .respond("second try");

    let client = Client::from_provider(Box::new(provider))
        .with_model("mock-model")
        .with_retry(quick(3));

    assert_eq!(client.complete("hi").await.unwrap(), "second try");
}

#[tokio::test]
async fn without_retry_returns_to_a_single_attempt() {
    let provider = MockProvider::new().fail_times(3, || CosmosError::Server {
        provider: "mock".into(),
        status: 503,
        message: "overloaded".into(),
    });

    let client = Client::from_provider(Box::new(provider))
        .with_retry(quick(3))
        .without_retry();

    assert!(client.retry_policy().is_none());
    assert!(client.completion(request()).await.is_err());
}

#[tokio::test]
async fn each_error_class_is_retried_or_not_per_its_classification() {
    // The plan's verification item, end to end through the client.
    let cases: Vec<(fn() -> CosmosError, bool)> = vec![
        (
            || CosmosError::Server {
                provider: "mock".into(),
                status: 503,
                message: "m".into(),
            },
            true,
        ),
        (
            || CosmosError::RateLimit {
                provider: "mock".into(),
                message: "m".into(),
                retry_after: None,
            },
            true,
        ),
        (
            || CosmosError::Timeout {
                provider: "mock".into(),
                elapsed: Duration::from_secs(1),
            },
            true,
        ),
        (
            || CosmosError::Authentication {
                provider: "mock".into(),
                message: "m".into(),
            },
            false,
        ),
        (
            || CosmosError::InvalidRequest {
                provider: "mock".into(),
                message: "m".into(),
                param: None,
            },
            false,
        ),
        (
            || CosmosError::ContextLength {
                provider: "mock".into(),
                limit: Some(128_000),
            },
            false,
        ),
        (
            || CosmosError::ContentFiltered {
                provider: "mock".into(),
                reason: "m".into(),
            },
            false,
        ),
        (
            || CosmosError::ModelNotFound {
                provider: "mock".into(),
                model: "m".into(),
            },
            false,
        ),
    ];

    for (build, should_retry) in cases {
        // One failure, then a response. A retried class reaches the response;
        // a non-retried class does not.
        let provider = MockProvider::new()
            .fail_times(1, build)
            .respond("recovered");
        let client = Client::from_provider(Box::new(provider)).with_retry(quick(3));

        let result = client.completion(request()).await;
        assert_eq!(
            result.is_ok(),
            should_retry,
            "expected retried={should_retry}, got {result:?}"
        );
    }
}
