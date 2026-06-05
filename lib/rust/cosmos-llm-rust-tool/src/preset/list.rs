use std::sync::Arc;

use cosmos_llm_virtual_filesystem::Filesystem;
use serde_json::json;

use crate::{ParameterType, ToolDefinition};

/// Creates a list-files preset tool bound to `filesystem`.
///
/// Lists all files in the virtual filesystem, optionally filtered by a glob
/// pattern. Uses simple `*` wildcard matching against the full file path.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_virtual_filesystem::Filesystem;
/// use cosmos_llm_tool::preset::list_tool;
/// use serde_json::json;
/// use std::sync::Arc;
///
/// let fs = Arc::new(Filesystem::build("/", |root| {
///     root.file("a.rs", Some(""), Default::default()).unwrap();
///     root.file("b.txt", Some(""), Default::default()).unwrap();
/// }));
///
/// let tool = list_tool(fs);
/// let r = tool.call(&json!({ "pattern": "*.rs" })).unwrap();
/// assert_eq!(r["count"], json!(1));
/// ```
pub fn list_tool(filesystem: Arc<Filesystem>) -> ToolDefinition {
    ToolDefinition::new("list")
        .description("List all files in the virtual filesystem with their paths and metadata")
        .param(
            "pattern",
            ParameterType::String,
            false,
            "Optional glob pattern to filter files (e.g. '*.rb', 'src/**/*.js')",
        )
        .handler(move |params| {
            let pattern = params["pattern"].as_str().map(|s| s.to_string());
            let all = filesystem.all_files("");

            let filtered: Vec<_> = if let Some(ref pat) = pattern {
                let re = build_glob_regex(pat)
                    .map_err(|e| format!("invalid glob pattern: {e}"))?;
                let match_basename = !pat.contains('/');
                all.into_iter()
                    .filter(|e| {
                        let candidate = if match_basename {
                            e.path.rsplit('/').next().unwrap_or(&e.path)
                        } else {
                            e.path.as_str()
                        };
                        re.is_match(candidate)
                    })
                    .collect()
            } else {
                all
            };

            let files: Vec<serde_json::Value> = filtered
                .iter()
                .map(|e| {
                    json!({
                        "path": e.path,
                        "name": e.file.name,
                        "size": e.file.content.as_ref().map(|c| c.len()).unwrap_or(0),
                        "attributes": e.file.attributes,
                    })
                })
                .collect();

            Ok(json!({
                "success": true,
                "files": files,
                "count": files.len(),
                "pattern": pattern,
            }))
        })
}

fn build_glob_regex(pat: &str) -> Result<regex::Regex, regex::Error> {
    let mut out = String::from("^");
    let mut chars = pat.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                if chars.peek() == Some(&'/') {
                    chars.next();
                }
                out.push_str(".*");
            }
            '*' => out.push_str("[^/]*"),
            '?' => out.push('.'),
            '.' | '+' | '^' | '$' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out.push('$');
    regex::Regex::new(&out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_fs() -> Arc<Filesystem> {
        Arc::new(Filesystem::build("/", |root| {
            root.file("a.rs", Some(""), Default::default()).unwrap();
            root.file("b.txt", Some(""), Default::default()).unwrap();
            root.directory("src", |s| {
                s.file("lib.rs", Some(""), Default::default()).unwrap();
            });
        }))
    }

    #[test]
    fn list_all() {
        let t = list_tool(make_fs());
        let r = t.call(&json!({})).unwrap();
        assert_eq!(r["count"], json!(3));
    }

    #[test]
    fn list_with_pattern() {
        let t = list_tool(make_fs());
        // *.rs matches any .rs basename — a.rs and src/lib.rs
        let r = t.call(&json!({ "pattern": "*.rs" })).unwrap();
        assert_eq!(r["count"], json!(2));
        // b.txt should not match
        let r2 = t.call(&json!({ "pattern": "*.txt" })).unwrap();
        assert_eq!(r2["count"], json!(1));
    }

    #[test]
    fn list_recursive_pattern() {
        let t = list_tool(make_fs());
        let r = t.call(&json!({ "pattern": "**/*.rs" })).unwrap();
        assert_eq!(r["count"], json!(2));
    }
}
