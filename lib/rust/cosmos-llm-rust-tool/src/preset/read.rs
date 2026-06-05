use std::sync::Arc;

use cosmos_llm_virtual_filesystem::Filesystem;
use serde_json::json;

use crate::{ParameterType, ToolDefinition};

/// Creates a read-file preset tool bound to `filesystem`.
///
/// The tool reads a file by path from the virtual filesystem, returning
/// content with line numbers. Supports `offset` and `limit` for partial reads.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_virtual_filesystem::Filesystem;
/// use cosmos_llm_tool::preset::read_tool;
/// use serde_json::json;
/// use std::sync::Arc;
///
/// let fs = Arc::new(Filesystem::build("/", |root| {
///     root.file("hello.txt", Some("line1\nline2\n"), Default::default()).unwrap();
/// }));
///
/// let tool = read_tool(fs);
/// let result = tool.call(&json!({ "file_path": "hello.txt" })).unwrap();
/// assert_eq!(result["success"], json!(true));
/// assert!(result["content"].as_str().unwrap().contains("line1"));
/// ```
pub fn read_tool(filesystem: Arc<Filesystem>) -> ToolDefinition {
    ToolDefinition::new("read")
        .description(
            "Read file contents from the virtual filesystem with options for specific lines",
        )
        .param(
            "file_path",
            ParameterType::String,
            true,
            "Path to the file (relative to virtual filesystem root)",
        )
        .param(
            "offset",
            ParameterType::Integer,
            false,
            "Starting line number, 0-based",
        )
        .param(
            "limit",
            ParameterType::Integer,
            false,
            "Number of lines to read (default: 2000)",
        )
        .handler(move |params| {
            let file_path = params["file_path"]
                .as_str()
                .ok_or("file_path must be a string")?
                .to_string();
            let offset = params["offset"].as_i64().unwrap_or(0) as usize;
            let limit = params["limit"].as_i64().unwrap_or(2000) as usize;

            let vf = match filesystem.find_file(&file_path) {
                Some(f) => f,
                None => {
                    return Ok(json!({
                        "success": false,
                        "error": "File not found in virtual filesystem",
                        "file_path": file_path,
                    }))
                }
            };

            let content = match &vf.content {
                Some(c) => c,
                None => {
                    return Ok(json!({
                        "success": false,
                        "error": "File content is nil",
                        "file_path": file_path,
                    }))
                }
            };

            // Binary detection
            if content.contains('\x00') {
                return Ok(json!({
                    "success": true,
                    "file_path": file_path,
                    "content": content,
                    "total_lines": 0,
                    "read_lines": 1,
                    "start_line": 1,
                    "end_line": 1,
                    "size": content.len(),
                    "attributes": vf.attributes,
                }));
            }

            let lines: Vec<&str> = content.lines().collect();
            let total_lines = lines.len();
            let start = offset.min(total_lines);
            let end = (start + limit).min(total_lines);
            let selected = &lines[start..end];
            let read_lines = selected.len();

            let width = total_lines.to_string().len().max(1);
            let formatted: String = selected
                .iter()
                .enumerate()
                .map(|(i, line)| {
                    let ln = start + i + 1;
                    format!("{:>width$}\t{}\n", ln, line, width = width)
                })
                .collect();

            let start_line = start + 1;
            let end_line = if read_lines > 0 {
                start_line + read_lines - 1
            } else {
                start_line.saturating_sub(1)
            };

            Ok(json!({
                "success": true,
                "file_path": file_path,
                "content": formatted,
                "total_lines": total_lines,
                "read_lines": read_lines,
                "start_line": start_line,
                "end_line": end_line,
                "size": content.len(),
                "attributes": vf.attributes,
            }))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fs_with_file(name: &str, content: &str) -> Arc<Filesystem> {
        Arc::new(Filesystem::build("/", |root| {
            root.file(name, Some(content), Default::default()).unwrap();
        }))
    }

    #[test]
    fn read_existing_file() {
        let fs = fs_with_file("a.txt", "hello\nworld\n");
        let t = read_tool(fs);
        let r = t.call(&json!({ "file_path": "a.txt" })).unwrap();
        assert_eq!(r["success"], json!(true));
        assert!(r["content"].as_str().unwrap().contains("hello"));
        assert_eq!(r["total_lines"], json!(2));
    }

    #[test]
    fn read_missing_file() {
        let fs = Arc::new(Filesystem::new("/"));
        let t = read_tool(fs);
        let r = t.call(&json!({ "file_path": "nope.txt" })).unwrap();
        assert_eq!(r["success"], json!(false));
    }

    #[test]
    fn read_with_offset_and_limit() {
        let fs = fs_with_file("lines.txt", "a\nb\nc\nd\ne\n");
        let t = read_tool(fs);
        let r = t
            .call(&json!({ "file_path": "lines.txt", "offset": 1, "limit": 2 }))
            .unwrap();
        assert_eq!(r["read_lines"], json!(2));
        assert_eq!(r["start_line"], json!(2));
    }
}
