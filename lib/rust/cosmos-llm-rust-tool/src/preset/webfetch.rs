use reqwest;
use serde_json::json;

use crate::{ParameterType, ToolDefinition};

/// Creates a web-fetch preset tool.
///
/// Performs an HTTP GET request and returns the response body as a string.
/// This tool does not require a virtual filesystem.
///
/// Note: this is a synchronous wrapper that requires a Tokio runtime at call
/// time. In an async context, run it inside `tokio::task::spawn_blocking`.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_tool::preset::webfetch_tool;
///
/// let tool = webfetch_tool();
/// assert_eq!(tool.name, "webfetch");
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
        .handler(|params| {
            let url = params["url"]
                .as_str()
                .ok_or("url must be a string")?
                .to_string();

            // Validate URL format minimally
            if !url.starts_with("http://") && !url.starts_with("https://") {
                return Ok(json!({
                    "success": false,
                    "error": "URL must start with http:// or https://",
                    "url": url,
                }));
            }

            // Runtime must already exist (caller's responsibility)
            let result = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    let client = reqwest::Client::new();
                    let resp = client
                        .get(&url)
                        .header("User-Agent", "cosmos-llm-tool/0.1")
                        .send()
                        .await?;
                    let status = resp.status().as_u16();
                    let body = resp.text().await?;
                    Ok::<(u16, String), reqwest::Error>((status, body))
                })
            });

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
}
