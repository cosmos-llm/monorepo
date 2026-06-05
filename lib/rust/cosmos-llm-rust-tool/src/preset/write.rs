use std::sync::Arc;

use cosmos_llm_virtual_filesystem::Filesystem;
use serde_json::json;

use crate::{ParameterType, ToolDefinition};

/// Creates a write-file preset tool bound to `filesystem`.
///
/// The tool validates write parameters and checks whether the file already
/// exists, but does not mutate the filesystem — the caller is responsible
/// for applying the change. Returns metadata describing the operation.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_virtual_filesystem::Filesystem;
/// use cosmos_llm_tool::preset::write_tool;
/// use serde_json::json;
/// use std::sync::Arc;
///
/// let fs = Arc::new(Filesystem::new("/"));
/// let tool = write_tool(fs);
/// let result = tool.call(&json!({
///     "file_path": "new.txt",
///     "content": "hello",
/// })).unwrap();
/// assert_eq!(result["success"], json!(true));
/// assert_eq!(result["created"], json!(true));
/// ```
pub fn write_tool(filesystem: Arc<Filesystem>) -> ToolDefinition {
    ToolDefinition::new("write")
        .description("Write content to a file in the virtual filesystem")
        .param(
            "file_path",
            ParameterType::String,
            true,
            "Path to the file (relative to virtual filesystem root)",
        )
        .param(
            "content",
            ParameterType::String,
            true,
            "Content to write to the file",
        )
        .handler(move |params| {
            let file_path = params["file_path"]
                .as_str()
                .ok_or("file_path must be a string")?
                .to_string();
            let content = params["content"]
                .as_str()
                .ok_or("content must be a string")?
                .to_string();

            let existing = filesystem.find_file(&file_path);
            let previous_size = existing.and_then(|f| f.content.as_ref()).map(|c| c.len());

            Ok(json!({
                "success": true,
                "file_path": file_path,
                "content": content,
                "size": content.len(),
                "created": existing.is_none(),
                "updated": existing.is_some(),
                "previous_size": previous_size,
            }))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn write_new_file() {
        let fs = Arc::new(Filesystem::new("/"));
        let t = write_tool(fs);
        let r = t
            .call(&json!({ "file_path": "x.txt", "content": "hi" }))
            .unwrap();
        assert_eq!(r["success"], json!(true));
        assert_eq!(r["created"], json!(true));
        assert_eq!(r["size"], json!(2));
    }

    #[test]
    fn write_existing_file() {
        let fs = Arc::new(Filesystem::build("/", |root| {
            root.file("x.txt", Some("old"), Default::default()).unwrap();
        }));
        let t = write_tool(fs);
        let r = t
            .call(&json!({ "file_path": "x.txt", "content": "new" }))
            .unwrap();
        assert_eq!(r["updated"], json!(true));
        assert_eq!(r["previous_size"], json!(3));
    }
}
