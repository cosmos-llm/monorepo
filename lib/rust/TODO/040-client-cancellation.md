# 040 — Cancellation

**Status:** implemented — `CompletionRequest::with_cancel(CancellationToken)` (re-exported as `cosmos_llm::CancellationToken`), `CosmosError::Cancelled`, `Client` races completions (including the retry loop and its backoff) and streams against the token. Not deferred; calling a bare `Provider` directly does not honour the token by design.

**Crate:** `cosmos-llm` · **Depends on:** nothing · **Blocks:** 090

## Problem

When the user hits Esc, the in-flight request has to stop, both to free the
UI and to stop paying for tokens. Today:

- `Client::completion` / `chat` futures can be dropped, which aborts the HTTP
  request. That works, but only if the caller owns the future, and `AgentLoop`
  hides it.
- `CompletionStream` can be dropped too, but the retry wrapper inside
  `Client` may already be sleeping on a backoff and will start a fresh
  attempt if it is polled again.
- Nothing distinguishes "cancelled" from any other error.

## Design

Use `tokio_util::sync::CancellationToken`. It is cheap to clone, supports
parent/child relationships (cancel a turn → cancels its tools), and is what
codex and goose use.

```rust
impl CompletionRequest {
    pub fn with_cancel(self, token: CancellationToken) -> Self;
}
```

The token goes on the request rather than as a parameter so `Provider`'s
signature, and every out-of-tree provider, stays the same.

Inside the client:

- `completion`: `tokio::select!` the provider future against
  `token.cancelled()`. Return `CosmosError::Cancelled`.
- Retry: check the token before each attempt, and race the backoff sleep
  against it. A cancel must never start another attempt.
- `stream_completion`: wrap the returned stream so it yields
  `Err(Cancelled)` once and then ends when the token fires. Dropping the inner
  stream closes the connection.

`CosmosError::Cancelled` must return `false` from `is_retryable()`.

### Partial results

A stream cancelled halfway still has useful partial text. That is the
caller's concern: the accumulator already holds what arrived. 090 decides
what to do with it.

## Tests

- Mock provider that sleeps; cancel after 10 ms; `completion` returns
  `Cancelled` well before the sleep ends (use `tokio::time::pause`).
- Retry policy with three attempts, first attempt fails retryably, cancel
  during backoff → no second attempt is made (count calls on the mock).
- Stream yields two chunks, cancel → next item is `Err(Cancelled)`, then
  `None`.

## References

- `codex/codex-rs/core/src/tasks/`: turn-level cancellation tokens.
- `goose/crates/goose-agent`: `CancellationToken` threaded through `reply`.
