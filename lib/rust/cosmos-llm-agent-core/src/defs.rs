//! Shared plumbing for definition files (230, 240): markdown frontmatter,
//! key normalisation, and directory walking.
//!
//! Commands, agents and hooks all come from `.trongo` files (evaluated in the
//! sandbox of [`eval`](crate::eval)) or markdown files with a small
//! frontmatter block. Both end as a [`serde_json::Value`] that
//! `serde_json::from_value` lowers into the IR (005), so nothing downstream
//! knows which front end a definition came from.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::error::{CoreError, Result};
use crate::eval::ConfigEvaluator;

/// Splits `text` into frontmatter and body.
///
/// The frontmatter is the block between a leading `---` line and the next
/// `---` line. Supported values: bare or quoted strings, `[a, b]` lists, `-`
/// block lists, booleans and integers. Keys have `-` turned into `_`. Without
/// frontmatter the map is empty and the body is the whole text.
pub(crate) fn parse_frontmatter(text: &str) -> (Map<String, Value>, String) {
    let mut map = Map::new();
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.lines();
    if lines.next().map(str::trim_end) != Some("---") {
        return (map, text.to_owned());
    }
    let mut fm: Vec<&str> = Vec::new();
    let mut closed = false;
    let mut body_lines: Vec<&str> = Vec::new();
    for line in lines.by_ref() {
        if line.trim_end() == "---" {
            closed = true;
            break;
        }
        fm.push(line);
    }
    if !closed {
        return (map, text.to_owned());
    }
    body_lines.extend(lines);

    let mut i = 0;
    while i < fm.len() {
        let line = fm[i];
        i += 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().replace('-', "_");
        let value = value.trim();
        if value.is_empty() {
            let mut items = Vec::new();
            while i < fm.len() {
                match fm[i].trim().strip_prefix("- ") {
                    Some(item) => {
                        items.push(scalar(item.trim()));
                        i += 1;
                    }
                    None => break,
                }
            }
            map.insert(
                key,
                if items.is_empty() {
                    Value::String(String::new())
                } else {
                    Value::Array(items)
                },
            );
        } else if let Some(inner) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
            let items = inner
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(scalar)
                .collect();
            map.insert(key, Value::Array(items));
        } else {
            map.insert(key, scalar(value));
        }
    }
    let mut body = body_lines.join("\n");
    if text.ends_with('\n') {
        body.push('\n');
    }
    (map, body.trim_start_matches('\n').to_owned())
}

fn scalar(s: &str) -> Value {
    let s = s.trim();
    if s.len() >= 2 {
        let b = s.as_bytes();
        if (b[0] == b'"' && b[s.len() - 1] == b'"') || (b[0] == b'\'' && b[s.len() - 1] == b'\'') {
            return Value::String(s[1..s.len() - 1].to_owned());
        }
    }
    match s {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        _ => {}
    }
    if let Ok(n) = s.parse::<i64>() {
        return Value::from(n);
    }
    Value::String(s.to_owned())
}

/// Turns `-` into `_` in every object key, so `:max-steps` and `max_steps`
/// are one spelling.
pub(crate) fn normalize_keys(v: Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.into_iter()
                .map(|(k, v)| (k.replace('-', "_"), normalize_keys(v)))
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.into_iter().map(normalize_keys).collect()),
        other => other,
    }
}

/// Makes a list-valued field accept `"a, b"` as well as `["a", "b"]`.
pub(crate) fn listify(map: &mut Map<String, Value>, key: &str) {
    if let Some(Value::String(s)) = map.get(key) {
        let items = s
            .split([',', ' '])
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(|p| Value::String(p.to_owned()))
            .collect();
        map.insert(key.to_owned(), Value::Array(items));
    }
}

/// A definition file found under a directory.
#[derive(Debug, Clone)]
pub(crate) struct DefFile {
    /// Path relative to the directory, without extension, `/` as `:`.
    pub name: String,
    pub path: PathBuf,
    pub kind: DefKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DefKind {
    Trongo,
    Markdown,
}

/// Lists `.trongo` and `.md` files under `dir` (depth at most 4), sorted by
/// name, `.trongo` before `.md`. Hidden entries are skipped.
pub(crate) fn walk_defs(dir: &Path) -> Vec<DefFile> {
    let mut out = Vec::new();
    walk(dir, dir, 0, &mut out);
    out.sort_by(|a, b| {
        (&a.name, a.kind == DefKind::Markdown).cmp(&(&b.name, b.kind == DefKind::Markdown))
    });
    out
}

fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<DefFile>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if file_name.starts_with('.') {
            continue;
        }
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            if depth < 4 {
                walk(root, &path, depth + 1, out);
            }
            continue;
        }
        let kind = match path.extension().and_then(|e| e.to_str()) {
            Some("trongo") => DefKind::Trongo,
            Some("md") => DefKind::Markdown,
            _ => continue,
        };
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let name = rel
            .with_extension("")
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(":");
        out.push(DefFile { name, path, kind });
    }
}

/// Evaluates a `.trongo` definition file; the sandbox is its directory, so
/// `(read-file "review.prompt.erb")` finds a sibling.
pub(crate) fn eval_def(ev: &ConfigEvaluator, path: &Path) -> Result<Value> {
    let dir = path
        .parent()
        .ok_or_else(|| CoreError::Config(format!("{}: no parent directory", path.display())))?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| CoreError::Config(format!("{}: bad file name", path.display())))?;
    ev.eval_file(dir, name).map(normalize_keys)
}

/// Reads a text file of at most 1 MiB.
pub(crate) fn read_text(path: &Path) -> Result<String> {
    let meta = std::fs::metadata(path).map_err(|e| CoreError::io(path, e))?;
    if meta.len() > 1024 * 1024 {
        return Err(CoreError::Config(format!(
            "{} is over 1 MiB",
            path.display()
        )));
    }
    std::fs::read_to_string(path).map_err(|e| CoreError::io(path, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_forms() {
        let (m, body) = parse_frontmatter(
            "---\ndescription: Review it\nargs: \"[focus]\"\nallowed-tools: [read, grep]\nmax-steps: 7\nflag: true\ntools:\n  - a\n  - b\n---\nBody $ARGUMENTS\n",
        );
        assert_eq!(m["description"], "Review it");
        assert_eq!(m["args"], "[focus]");
        assert_eq!(m["allowed_tools"], serde_json::json!(["read", "grep"]));
        assert_eq!(m["max_steps"], 7);
        assert_eq!(m["flag"], true);
        assert_eq!(m["tools"], serde_json::json!(["a", "b"]));
        assert_eq!(body, "Body $ARGUMENTS\n");
    }

    #[test]
    fn no_frontmatter_is_all_body() {
        let (m, body) = parse_frontmatter("just text\n---\nnot front\n");
        assert!(m.is_empty());
        assert!(body.starts_with("just text"));
        let (m, body) = parse_frontmatter("---\nunclosed: yes\nbody");
        assert!(m.is_empty());
        assert!(body.starts_with("---"));
    }

    #[test]
    fn keys_normalise() {
        let v = normalize_keys(serde_json::json!({"max-steps": 1, "a": [{"x-y": 2}]}));
        assert_eq!(v, serde_json::json!({"max_steps": 1, "a": [{"x_y": 2}]}));
    }
}
