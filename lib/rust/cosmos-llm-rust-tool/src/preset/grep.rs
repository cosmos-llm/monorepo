use std::sync::Arc;

use cosmos_llm_virtual_filesystem::Filesystem;
use regex::Regex;
use serde_json::json;

use crate::{ParameterType, ToolDefinition};

/// Creates a grep preset tool bound to `filesystem`.
///
/// Searches for a regex pattern across all (or a filtered subset of) files in
/// the virtual filesystem. Binary files (containing null bytes) are skipped.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_virtual_filesystem::Filesystem;
/// use cosmos_llm_tool::preset::grep_tool;
/// use serde_json::json;
/// use std::sync::Arc;
///
/// let fs = Arc::new(Filesystem::build("/", |root| {
///     root.file("a.rs", Some("// TODO: fix this\nfn main(){}\n"), Default::default()).unwrap();
/// }));
///
/// let tool = grep_tool(fs);
/// let r = tool.call(&json!({ "pattern": "TODO" })).unwrap();
/// assert_eq!(r["match_count"], json!(1));
/// ```
pub fn grep_tool(filesystem: Arc<Filesystem>) -> ToolDefinition {
    ToolDefinition::new("grep")
        .description(
            "Search for regex patterns in files within the virtual filesystem",
        )
        .param(
            "pattern",
            ParameterType::String,
            true,
            "Regex pattern to search for",
        )
        .param(
            "file_pattern",
            ParameterType::String,
            false,
            "Glob pattern to restrict which files are searched (e.g. '*.rb')",
        )
        .handler(move |params| {
            let search_pattern = params["pattern"]
                .as_str()
                .ok_or("pattern must be a string")?
                .to_string();
            let file_pattern = params["file_pattern"].as_str().map(|s| s.to_string());

            let regex = Regex::new(&search_pattern)
                .map_err(|e| format!("invalid regex pattern: {e}"))?;

            let all = filesystem.all_files("");

            let files_to_search: Vec<_> = if let Some(ref fpat) = file_pattern {
                let freg = file_glob_regex(fpat)
                    .map_err(|e| format!("invalid file pattern: {e}"))?;
                let match_basename = !fpat.contains('/');
                all.into_iter()
                    .filter(|e| {
                        let candidate = if match_basename {
                            e.path.rsplit('/').next().unwrap_or(&e.path)
                        } else {
                            e.path.as_str()
                        };
                        freg.is_match(candidate)
                    })
                    .collect()
            } else {
                all
            };

            let mut matches: Vec<serde_json::Value> = Vec::new();
            let mut files_searched = 0usize;
            let mut files_with_matches = 0usize;

            for entry in &files_to_search {
                let content = match &entry.file.content {
                    Some(c) if !c.contains('\x00') => c,
                    _ => continue,
                };

                files_searched += 1;
                let mut file_matched = false;

                for (idx, line) in content.lines().enumerate() {
                    for m in regex.find_iter(line) {
                        file_matched = true;
                        matches.push(json!({
                            "file": entry.path,
                            "line_number": idx + 1,
                            "content": line,
                            "match": m.as_str(),
                        }));
                    }
                }

                if file_matched {
                    files_with_matches += 1;
                }
            }

            Ok(json!({
                "success": true,
                "pattern": search_pattern,
                "file_pattern": file_pattern,
                "matches": matches,
                "match_count": matches.len(),
                "files_searched": files_searched,
                "files_with_matches": files_with_matches,
            }))
        })
}

fn file_glob_regex(pat: &str) -> Result<Regex, regex::Error> {
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
            '.' | '+' | '^' | '$' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out.push('$');
    Regex::new(&out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_fs() -> Arc<Filesystem> {
        Arc::new(Filesystem::build("/", |root| {
            root.file(
                "a.rs",
                Some("// TODO: fix\nfn main(){}\n"),
                Default::default(),
            )
            .unwrap();
            root.file("b.txt", Some("nothing here\n"), Default::default())
                .unwrap();
        }))
    }

    #[test]
    fn finds_match() {
        let t = grep_tool(make_fs());
        let r = t.call(&json!({ "pattern": "TODO" })).unwrap();
        assert_eq!(r["match_count"], json!(1));
        assert_eq!(r["files_with_matches"], json!(1));
    }

    #[test]
    fn file_pattern_filters() {
        let t = grep_tool(make_fs());
        let r = t
            .call(&json!({ "pattern": "nothing", "file_pattern": "*.rs" }))
            .unwrap();
        assert_eq!(r["match_count"], json!(0));
    }

    #[test]
    fn invalid_regex_fails() {
        let t = grep_tool(make_fs());
        let r = t.call(&json!({ "pattern": "[invalid" }));
        assert!(r.is_err() || r.unwrap()["success"] == json!(false));
    }
}
