use serde_json::json;

use crate::{ParameterType, ToolDefinition};

/// Creates a web-fetch preset tool.
///
/// Performs an HTTP GET and returns the response body as a string.
/// This tool does not require a virtual filesystem.
///
/// The handler is asynchronous, so it needs
/// [`Executor::execute_async`](crate::Executor::execute_async) or
/// [`ToolDefinition::call_async`] rather than the synchronous path. That is the
/// point: an HTTP fetch driven from a sync handler has to block a thread, and a
/// batch of them run through
/// [`Executor::execute_all`](crate::Executor::execute_all) would block as many
/// threads as there are fetches.
///
/// A request failure is reported in the returned JSON rather than as a tool
/// error, since a fetch that 404s is a useful answer for the model — it just
/// needs to be told.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_tool::preset::webfetch_tool;
///
/// let tool = webfetch_tool();
/// assert_eq!(tool.name, "webfetch");
/// assert!(tool.is_async());
/// ```
pub fn webfetch_tool() -> ToolDefinition {
    ToolDefinition::new("webfetch")
        .description("Fetch the content of a URL via HTTP GET")
        .param("url", ParameterType::String, true, "URL to fetch")
        .param(
            "format",
            ParameterType::String,
            false,
            "Response format: 'text' (default) or 'markdown'",
        )
        .async_handler(|params| {
            Box::pin(async move {
                let url = params["url"]
                    .as_str()
                    .ok_or("url must be a string")?
                    .to_owned();

                // Rejected before the request, not after: a bare hostname or a
                // `file://` path is a caller mistake, and letting reqwest
                // report it buries that in a transport error.
                if !url.starts_with("http://") && !url.starts_with("https://") {
                    return Ok(json!({
                        "success": false,
                        "error": "URL must start with http:// or https://",
                        "url": url,
                    }));
                }

                let client = reqwest::Client::new();
                let result = async {
                    let resp = client
                        .get(&url)
                        .header("User-Agent", "cosmos-llm-tool/0.1")
                        .send()
                        .await?;
                    let status = resp.status().as_u16();
                    let body = resp.text().await?;
                    Ok::<(u16, String), reqwest::Error>((status, body))
                }
                .await;

                match result {
                    Ok((status, body)) => Ok(json!({
                        "success": true,
                        "url": url,
                        "status": status,
                        "content": body,
                    })),
                    Err(e) => Ok(json!({
                        "success": false,
                        "url": url,
                        "error": e.to_string(),
                    })),
                }
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Executor;

    #[test]
    fn is_an_async_tool() {
        // The proof that 020.010 landed: no runtime workaround, so the sync
        // path refuses it rather than blocking.
        let tool = webfetch_tool();
        assert!(tool.is_async());
        assert!(matches!(
            Executor::execute(&tool, &serde_json::json!({"url": "http://example.com"})),
            Err(crate::ToolError::AsyncHandler(_))
        ));
    }

    #[tokio::test]
    async fn rejects_a_non_http_url_without_making_a_request() {
        let tool = webfetch_tool();
        let out = Executor::execute_async(&tool, json!({"url": "file:///etc/passwd"}))
            .await
            .unwrap();
        assert_eq!(out["success"], false);
        assert!(out["error"].as_str().unwrap().contains("http://"));
    }

    #[tokio::test]
    async fn requires_a_url() {
        let tool = webfetch_tool();
        assert!(Executor::execute_async(&tool, json!({})).await.is_err());
    }

    #[tokio::test]
    async fn a_connection_failure_is_reported_in_the_result() {
        // Port 1 on loopback refuses connections, so this needs no network.
        let tool = webfetch_tool();
        let out = Executor::execute_async(&tool, json!({"url": "http://127.0.0.1:1/"}))
            .await
            .unwrap();
        assert_eq!(out["success"], false);
        assert!(out["error"].is_string());
    }

    #[tokio::test(start_paused = true)]
    async fn honors_a_timeout() {
        // A tool that hangs must not hang the loop; the timeout is enforced by
        // the executor, not by the handler.
        let tool = webfetch_tool().timeout(std::time::Duration::from_millis(1));
        // 10.255.255.1 is a reserved address that black-holes rather than
        // refusing, so the request hangs until the timeout fires.
        let err = Executor::execute_async(&tool, json!({"url": "http://10.255.255.1/"}))
            .await
            .unwrap_err();
        assert!(matches!(err, crate::ToolError::Timeout { .. }));
    }
}
