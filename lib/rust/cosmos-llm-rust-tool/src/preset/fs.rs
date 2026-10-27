//! The five filesystem presets over any [`ToolFs`].
//!
//! These are the same tools as [`read_tool`](super::read_tool) and friends,
//! but bound to a [`ToolFs`] so one set of schemas, descriptions and output
//! shapes serves the in-memory VFS and a real working tree. Paths in results
//! are relative to the filesystem root, with no leading slash.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use regex::Regex;
use serde_json::{json, Value};

use super::glob::glob_to_regex;
use crate::fs::{ToolFs, WalkOptions};
use crate::{ParameterType, ToolDefinition};

/// Renders file bytes the way the `read` tool reports them.
///
/// Valid UTF-8 becomes numbered lines (`offset` is 0-based); anything else is
/// reported as binary with its size. Shared with other crates' `read` tools so
/// the output shape stays the same everywhere.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_tool::preset::render_read;
///
/// let r = render_read("a.txt", b"x\ny\n", 0, 2000);
/// assert_eq!(r["total_lines"], 2);
/// assert_eq!(render_read("b", &[0xff, 0x00], 0, 10)["binary"], true);
/// ```
pub fn render_read(file_path: &str, bytes: &[u8], offset: usize, limit: usize) -> Value {
    let content = match std::str::from_utf8(bytes) {
        Ok(c) if !c.contains('\0') => c,
        _ => {
            return json!({
                "success": true,
                "file_path": file_path,
                "binary": true,
                "content": format!("binary file, {} bytes", bytes.len()),
                "size": bytes.len(),
            })
        }
    };
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();
    let start = offset.min(total_lines);
    let end = start.saturating_add(limit).min(total_lines);
    let selected = &lines[start..end];
    let width = total_lines.to_string().len().max(1);
    let formatted: String = selected
        .iter()
        .enumerate()
        .map(|(i, line)| format!("{:>width$}\t{}\n", start + i + 1, line, width = width))
        .collect();
    let start_line = start + 1;
    let end_line = if selected.is_empty() {
        start_line.saturating_sub(1)
    } else {
        start_line + selected.len() - 1
    };
    json!({
        "success": true,
        "file_path": file_path,
        "content": formatted,
        "total_lines": total_lines,
        "read_lines": selected.len(),
        "start_line": start_line,
        "end_line": end_line,
        "size": bytes.len(),
    })
}

fn failure(file_path: &str, error: impl std::fmt::Display) -> Value {
    json!({ "success": false, "error": error.to_string(), "file_path": file_path })
}

/// Creates a `read` tool over `fs`.
///
/// # Examples
///
/// ```rust
/// use std::sync::Arc;
/// use cosmos_llm_tool::fs::VfsFs;
/// use cosmos_llm_tool::fs::ToolFs;
/// use cosmos_llm_tool::preset::read_tool_fs;
/// use serde_json::json;
///
/// let fs = Arc::new(VfsFs::empty());
/// fs.write(std::path::Path::new("a.txt"), b"hello\n").unwrap();
/// let r = read_tool_fs(fs).call(&json!({ "file_path": "a.txt" })).unwrap();
/// assert_eq!(r["total_lines"], 1);
/// ```
pub fn read_tool_fs(fs: Arc<dyn ToolFs>) -> ToolDefinition {
    ToolDefinition::new("read")
        .description("Read file contents with line numbers, optionally a range of lines")
        .param(
            "file_path",
            ParameterType::String,
            true,
            "Path to the file (absolute or relative to the workspace root)",
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
                .ok_or("file_path must be a string")?;
            let offset = params["offset"].as_i64().unwrap_or(0).max(0) as usize;
            let limit = params["limit"].as_i64().unwrap_or(2000).max(0) as usize;
            Ok(match fs.read(Path::new(file_path)) {
                Ok(bytes) => render_read(file_path, &bytes, offset, limit),
                Err(e) => failure(file_path, e),
            })
        })
}

/// Creates a `write` tool over `fs`: full overwrite or create, no guards.
///
/// The coding-tools crate wraps this with a read-before-write check.
///
/// # Examples
///
/// ```rust
/// use std::sync::Arc;
/// use cosmos_llm_tool::fs::VfsFs;
/// use cosmos_llm_tool::preset::write_tool_fs;
/// use serde_json::json;
///
/// let fs = Arc::new(VfsFs::empty());
/// let r = write_tool_fs(fs)
///     .call(&json!({ "file_path": "a/b.txt", "content": "hi" }))
///     .unwrap();
/// assert_eq!(r["created"], true);
/// ```
pub fn write_tool_fs(fs: Arc<dyn ToolFs>) -> ToolDefinition {
    ToolDefinition::new("write")
        .description("Write content to a file, creating it and any parent directories")
        .param(
            "file_path",
            ParameterType::String,
            true,
            "Path to the file (absolute or relative to the workspace root)",
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
                .ok_or("file_path must be a string")?;
            let content = params["content"]
                .as_str()
                .ok_or("content must be a string")?;
            let path = Path::new(file_path);
            let previous = fs.metadata(path).ok().filter(|m| !m.is_dir).map(|m| m.len);
            Ok(match fs.write(path, content.as_bytes()) {
                Ok(()) => json!({
                    "success": true,
                    "file_path": file_path,
                    "size": content.len(),
                    "created": previous.is_none(),
                    "updated": previous.is_some(),
                    "previous_size": previous,
                }),
                Err(e) => failure(file_path, e),
            })
        })
}

fn walk_files(fs: &dyn ToolFs) -> Vec<(PathBuf, String)> {
    let mut v: Vec<(PathBuf, String)> = fs
        .walk(Path::new(""), &WalkOptions::default())
        .filter_map(Result::ok)
        .map(|p| {
            let rel = fs.relativize(&p);
            let shown = rel.to_string_lossy().replace('\\', "/");
            (p, shown)
        })
        .collect();
    v.sort_by(|a, b| a.1.cmp(&b.1));
    v
}

fn matches_glob(re: &Regex, basename_only: bool, shown: &str) -> bool {
    let candidate = if basename_only {
        shown.rsplit('/').next().unwrap_or(shown)
    } else {
        shown
    };
    re.is_match(candidate)
}

/// Creates a `glob` tool over `fs`.
///
/// # Examples
///
/// ```rust
/// use std::sync::Arc;
/// use cosmos_llm_tool::fs::{ToolFs, VfsFs};
/// use cosmos_llm_tool::preset::glob_tool_fs;
/// use serde_json::json;
///
/// let fs = Arc::new(VfsFs::empty());
/// fs.write(std::path::Path::new("src/lib.rs"), b"").unwrap();
/// let r = glob_tool_fs(fs).call(&json!({ "pattern": "**/*.rs" })).unwrap();
/// assert_eq!(r["count"], 1);
/// ```
pub fn glob_tool_fs(fs: Arc<dyn ToolFs>) -> ToolDefinition {
    ToolDefinition::new("glob")
        .description("Find files matching a glob pattern")
        .param(
            "pattern",
            ParameterType::String,
            true,
            "Glob pattern (e.g. '**/*.rs', 'src/*.{js,ts}')",
        )
        .handler(move |params| {
            let pattern = params["pattern"]
                .as_str()
                .ok_or("pattern must be a string")?;
            let re = glob_to_regex(pattern).map_err(|e| format!("invalid glob pattern: {e}"))?;
            let basename_only = !pattern.contains('/');
            let paths: Vec<String> = walk_files(fs.as_ref())
                .into_iter()
                .filter(|(_, shown)| matches_glob(&re, basename_only, shown))
                .map(|(_, shown)| shown)
                .collect();
            Ok(json!({
                "success": true,
                "pattern": pattern,
                "count": paths.len(),
                "paths": paths,
            }))
        })
}

/// Creates a `list` tool over `fs`.
///
/// # Examples
///
/// ```rust
/// use std::sync::Arc;
/// use cosmos_llm_tool::fs::{ToolFs, VfsFs};
/// use cosmos_llm_tool::preset::list_tool_fs;
/// use serde_json::json;
///
/// let fs = Arc::new(VfsFs::empty());
/// fs.write(std::path::Path::new("a.rs"), b"").unwrap();
/// fs.write(std::path::Path::new("b.txt"), b"").unwrap();
/// let r = list_tool_fs(fs).call(&json!({ "pattern": "*.rs" })).unwrap();
/// assert_eq!(r["count"], 1);
/// ```
pub fn list_tool_fs(fs: Arc<dyn ToolFs>) -> ToolDefinition {
    ToolDefinition::new("list")
        .description("List all files with their paths and sizes")
        .param(
            "pattern",
            ParameterType::String,
            false,
            "Optional glob pattern to filter files (e.g. '*.rb', 'src/**/*.js')",
        )
        .handler(move |params| {
            let re = match params["pattern"].as_str() {
                Some(p) => Some((
                    glob_to_regex(p).map_err(|e| format!("invalid glob pattern: {e}"))?,
                    !p.contains('/'),
                )),
                None => None,
            };
            let files: Vec<Value> = walk_files(fs.as_ref())
                .into_iter()
                .filter(|(_, shown)| {
                    re.as_ref()
                        .map_or(true, |(re, base)| matches_glob(re, *base, shown))
                })
                .map(|(p, shown)| {
                    let size = fs.metadata(&p).map(|m| m.len).unwrap_or(0);
                    let name = shown.rsplit('/').next().unwrap_or(&shown).to_string();
                    json!({ "path": shown, "name": name, "size": size })
                })
                .collect();
            Ok(json!({ "success": true, "count": files.len(), "files": files }))
        })
}

/// Creates a `grep` tool over `fs`: regex search across files, line by line.
///
/// # Examples
///
/// ```rust
/// use std::sync::Arc;
/// use cosmos_llm_tool::fs::{ToolFs, VfsFs};
/// use cosmos_llm_tool::preset::grep_tool_fs;
/// use serde_json::json;
///
/// let fs = Arc::new(VfsFs::empty());
/// fs.write(std::path::Path::new("a.rs"), b"// TODO: x\n").unwrap();
/// let r = grep_tool_fs(fs).call(&json!({ "pattern": "TODO" })).unwrap();
/// assert_eq!(r["match_count"], 1);
/// ```
pub fn grep_tool_fs(fs: Arc<dyn ToolFs>) -> ToolDefinition {
    ToolDefinition::new("grep")
        .description("Search for regex patterns in files")
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
            let pattern = params["pattern"]
                .as_str()
                .ok_or("pattern must be a string")?;
            let file_pattern = params["file_pattern"].as_str();
            let regex = Regex::new(pattern).map_err(|e| format!("invalid regex pattern: {e}"))?;
            let fre = match file_pattern {
                Some(p) => Some((
                    glob_to_regex(p).map_err(|e| format!("invalid file pattern: {e}"))?,
                    !p.contains('/'),
                )),
                None => None,
            };

            let mut matches: Vec<Value> = Vec::new();
            let mut files_searched = 0usize;
            let mut files_with_matches = 0usize;
            for (p, shown) in walk_files(fs.as_ref()) {
                if let Some((re, base)) = &fre {
                    if !matches_glob(re, *base, &shown) {
                        continue;
                    }
                }
                let Ok(bytes) = fs.read(&p) else { continue };
                let Ok(content) = std::str::from_utf8(&bytes) else {
                    continue;
                };
                if content.contains('\0') {
                    continue;
                }
                files_searched += 1;
                let before = matches.len();
                for (idx, line) in content.lines().enumerate() {
                    for m in regex.find_iter(line) {
                        matches.push(json!({
                            "file": shown,
                            "line_number": idx + 1,
                            "content": line,
                            "match": m.as_str(),
                        }));
                    }
                }
                if matches.len() > before {
                    files_with_matches += 1;
                }
            }
            Ok(json!({
                "success": true,
                "pattern": pattern,
                "file_pattern": file_pattern,
                "match_count": matches.len(),
                "matches": matches,
                "files_searched": files_searched,
                "files_with_matches": files_with_matches,
            }))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::VfsFs;

    fn seeded() -> Arc<VfsFs> {
        let fs = Arc::new(VfsFs::empty());
        fs.write(Path::new("a.txt"), b"hello\nworld\n").unwrap();
        fs.write(Path::new("src/lib.rs"), b"// TODO: fix\nfn main(){}\n")
            .unwrap();
        fs
    }

    #[test]
    fn read_offset_limit_and_missing() {
        let fs = seeded();
        let t = read_tool_fs(fs);
        let r = t
            .call(&json!({ "file_path": "a.txt", "offset": 1, "limit": 1 }))
            .unwrap();
        assert_eq!(r["start_line"], 2);
        assert_eq!(r["read_lines"], 1);
        let r = t.call(&json!({ "file_path": "nope" })).unwrap();
        assert_eq!(r["success"], false);
    }

    #[test]
    fn write_then_overwrite() {
        let fs = seeded();
        let t = write_tool_fs(fs.clone());
        let r = t
            .call(&json!({ "file_path": "n/x.txt", "content": "hi" }))
            .unwrap();
        assert_eq!(r["created"], true);
        let r = t
            .call(&json!({ "file_path": "n/x.txt", "content": "hello" }))
            .unwrap();
        assert_eq!(r["updated"], true);
        assert_eq!(r["previous_size"], 2);
        assert_eq!(fs.read(Path::new("n/x.txt")).unwrap(), b"hello");
    }

    #[test]
    fn glob_list_grep() {
        let fs = seeded();
        let r = glob_tool_fs(fs.clone())
            .call(&json!({ "pattern": "**/*.rs" }))
            .unwrap();
        assert_eq!(r["paths"], json!(["src/lib.rs"]));
        let r = list_tool_fs(fs.clone()).call(&json!({})).unwrap();
        assert_eq!(r["count"], 2);
        let r = grep_tool_fs(fs)
            .call(&json!({ "pattern": "TODO", "file_pattern": "*.rs" }))
            .unwrap();
        assert_eq!(r["match_count"], 1);
        assert_eq!(r["matches"][0]["file"], "src/lib.rs");
    }
}
