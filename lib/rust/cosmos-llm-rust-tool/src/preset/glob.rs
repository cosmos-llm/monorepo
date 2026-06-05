use std::sync::Arc;

use cosmos_llm_virtual_filesystem::Filesystem;
use serde_json::json;

use crate::{ParameterType, ToolDefinition};

/// Creates a glob-pattern matching preset tool bound to `filesystem`.
///
/// Lists all files whose paths match the given glob pattern, supporting `*`,
/// `**`, and `?` wildcards.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_virtual_filesystem::Filesystem;
/// use cosmos_llm_tool::preset::glob_tool;
/// use serde_json::json;
/// use std::sync::Arc;
///
/// let fs = Arc::new(Filesystem::build("/", |root| {
///     root.directory("src", |s| {
///         s.file("lib.rs", Some(""), Default::default()).unwrap();
///     });
///     root.file("Cargo.toml", Some(""), Default::default()).unwrap();
/// }));
///
/// let tool = glob_tool(fs);
/// let r = tool.call(&json!({ "pattern": "**/*.rs" })).unwrap();
/// assert_eq!(r["count"], json!(1));
/// ```
pub fn glob_tool(filesystem: Arc<Filesystem>) -> ToolDefinition {
    ToolDefinition::new("glob")
        .description("Find files matching a glob pattern in the virtual filesystem")
        .param(
            "pattern",
            ParameterType::String,
            true,
            "Glob pattern (e.g. '**/*.rs', 'src/*.{js,ts}')",
        )
        .handler(move |params| {
            let pattern = params["pattern"]
                .as_str()
                .ok_or("pattern must be a string")?
                .to_string();

            let re = glob_to_regex(&pattern)
                .map_err(|e| format!("invalid glob pattern: {e}"))?;
            let match_basename = !pattern.contains('/');

            let all = filesystem.all_files("");
            let matched: Vec<serde_json::Value> = all
                .iter()
                .filter(|e| {
                    let candidate = if match_basename {
                        e.path.rsplit('/').next().unwrap_or(&e.path)
                    } else {
                        e.path.as_str()
                    };
                    re.is_match(candidate)
                })
                .map(|e| json!(e.path))
                .collect();

            Ok(json!({
                "success": true,
                "pattern": pattern,
                "paths": matched,
                "count": matched.len(),
            }))
        })
}

fn glob_to_regex(pat: &str) -> Result<regex::Regex, regex::Error> {
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
            '{' => {
                // {a,b,c} → (a|b|c)
                let mut group = String::from("(");
                let mut first = true;
                for gc in chars.by_ref() {
                    if gc == '}' {
                        break;
                    }
                    if gc == ',' {
                        group.push('|');
                        first = false;
                    } else {
                        if !first {
                        }
                        group.push(gc);
                    }
                }
                group.push(')');
                out.push_str(&group);
            }
            '.' | '+' | '^' | '$' | '(' | ')' | '[' | ']' | '|' | '\\' => {
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
            root.directory("src", |s| {
                s.file("lib.rs", Some(""), Default::default()).unwrap();
                s.file("main.rs", Some(""), Default::default()).unwrap();
            });
            root.file("Cargo.toml", Some(""), Default::default()).unwrap();
        }))
    }

    #[test]
    fn glob_all_rs() {
        let t = glob_tool(make_fs());
        let r = t.call(&json!({ "pattern": "**/*.rs" })).unwrap();
        assert_eq!(r["count"], json!(2));
    }

    #[test]
    fn glob_toml() {
        let t = glob_tool(make_fs());
        let r = t.call(&json!({ "pattern": "*.toml" })).unwrap();
        assert_eq!(r["count"], json!(1));
    }
}
