//! The tool definitions: `read`, `write`, `edit`, `multi_edit`, `glob`,
//! `grep`, all bound to one filesystem and one [`FileTracker`].

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cosmos_llm_tool::fs::ToolFs;
use cosmos_llm_tool::parameter::ParameterDef;
use cosmos_llm_tool::preset::render_read;
use cosmos_llm_tool::{ParameterType, Registry, ToolDefinition, ToolHints};
use serde_json::{json, Value};

use crate::diff::{file_diff, ChangeKind, FileDiff};
use crate::edit::{apply_edits, to_crlf, to_lf, uses_crlf, EditSpec};
use crate::search::{glob_tool, grep_tool, GrepBackend};
use crate::shell::{ShellConfig, ShellTools};
use crate::tracker::{FileTracker, SharedTracker};
use crate::DiskFs;

/// A change computed but not yet written.
///
/// [`plan_edit`] and [`plan_write`] are pure with respect to the disk, so an
/// approval UI can show [`diff`](EditPlan::diff) before anything changes;
/// [`commit_plan`] then writes exactly this content, after checking the file
/// is still what the plan was computed from.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct EditPlan {
    /// Normalized path (the tracker's key).
    pub path: PathBuf,
    /// Path as shown to the user.
    pub shown: String,
    /// Content the plan was computed from; `None` if the file is new.
    pub before: Option<Vec<u8>>,
    /// Content to write.
    pub after: Vec<u8>,
    /// The change, for display.
    pub diff: FileDiff,
    /// Places changed.
    pub replacements: usize,
    /// `true` if the match ignored whitespace differences.
    pub whitespace_fallback: bool,
}

fn text_of(bytes: &[u8], shown: &str) -> Result<String, String> {
    String::from_utf8(bytes.to_vec()).map_err(|_| format!("{shown} is not a UTF-8 text file"))
}

fn guard(fs: &dyn ToolFs, tracker: &FileTracker, path: &Path) -> Result<(), String> {
    tracker.check_fresh(fs, path).map_err(|e| e.to_string())
}

/// Plans `edits` against `file_path`. See the `edit` and `multi_edit` tools.
///
/// # Errors
///
/// A message written for the model: stale or unread file, no match, several
/// matches, and so on. For multiple edits the failing index is named, and
/// nothing is applied.
pub fn plan_edit(
    fs: &dyn ToolFs,
    tracker: &FileTracker,
    file_path: &str,
    edits: &[EditSpec],
) -> Result<EditPlan, String> {
    if edits.is_empty() {
        return Err("edits must not be empty".into());
    }
    let path = fs.normalize(Path::new(file_path));
    let shown = fs.relativize(&path).to_string_lossy().into_owned();
    let meta = fs.metadata(&path).ok();
    if meta.as_ref().is_some_and(|m| m.is_dir) {
        return Err(format!("{shown} is a directory"));
    }
    let (before, crlf, current) = match meta {
        Some(_) => {
            let bytes = fs.read(&path).map_err(|e| format!("{shown}: {e}"))?;
            guard(fs, tracker, &path)?;
            let text = text_of(&bytes, &shown)?;
            let crlf = uses_crlf(&text);
            (Some(bytes), crlf, to_lf(&text))
        }
        None => {
            if edits[0].old_string.is_empty() {
                (None, false, String::new())
            } else {
                return Err(format!(
                    "{shown} does not exist. To create it, use an empty old_string."
                ));
            }
        }
    };
    let applied = apply_edits(&current, edits).map_err(|(i, e)| {
        if edits.len() > 1 {
            format!("edit {i} failed (no changes were made): {e}")
        } else {
            e.to_string()
        }
    })?;
    let after_text = if crlf {
        to_crlf(&applied.content)
    } else {
        applied.content
    };
    let before_text = before
        .as_deref()
        .map(|b| String::from_utf8_lossy(b).into_owned());
    let diff = file_diff(&shown, before_text.as_deref(), Some(&after_text));
    Ok(EditPlan {
        path,
        shown,
        before,
        after: after_text.into_bytes(),
        diff,
        replacements: applied.replacements,
        whitespace_fallback: applied.whitespace_fallback,
    })
}

/// Plans a full overwrite or create of `file_path`.
///
/// Overwriting an existing file needs the read-before-write guard to pass;
/// creating a new one does not.
pub fn plan_write(
    fs: &dyn ToolFs,
    tracker: &FileTracker,
    file_path: &str,
    content: &str,
) -> Result<EditPlan, String> {
    let path = fs.normalize(Path::new(file_path));
    let shown = fs.relativize(&path).to_string_lossy().into_owned();
    let meta = fs.metadata(&path).ok();
    if meta.as_ref().is_some_and(|m| m.is_dir) {
        return Err(format!("{shown} is a directory"));
    }
    let before = match meta {
        Some(_) => {
            let bytes = fs.read(&path).map_err(|e| format!("{shown}: {e}"))?;
            guard(fs, tracker, &path)?;
            Some(bytes)
        }
        None => None,
    };
    let diff = match before.as_deref().map(std::str::from_utf8) {
        Some(Err(_)) => FileDiff::binary(&shown, ChangeKind::Modified),
        Some(Ok(old)) => file_diff(&shown, Some(old), Some(content)),
        None => file_diff(&shown, None, Some(content)),
    };
    Ok(EditPlan {
        path,
        shown,
        before,
        after: content.as_bytes().to_vec(),
        diff,
        replacements: 1,
        whitespace_fallback: false,
    })
}

/// Writes a plan and records it in the tracker.
///
/// Re-reads the file first and fails if it is no longer what the plan was
/// computed from.
pub fn commit_plan(
    fs: &dyn ToolFs,
    tracker: &mut FileTracker,
    plan: &EditPlan,
) -> Result<(), String> {
    let now = fs.read(&plan.path).ok();
    if now != plan.before {
        return Err(format!(
            "{} changed while the edit was being prepared. Read it again before editing.",
            plan.shown
        ));
    }
    fs.write(&plan.path, &plan.after)
        .map_err(|e| format!("could not write {}: {e}", plan.shown))?;
    let turn = tracker.turn();
    tracker.record_write(fs, turn, &plan.path, plan.before.clone(), Some(&plan.after));
    Ok(())
}

fn plan_result(plan: &EditPlan, verb: &str) -> Value {
    let mut v = json!({
        "success": true,
        "file_path": plan.shown,
        "message": format!(
            "{verb} {} (+{} \u{2212}{})",
            plan.shown, plan.diff.added, plan.diff.removed
        ),
        "added": plan.diff.added,
        "removed": plan.diff.removed,
        "replacements": plan.replacements,
        "diff": plan.diff.unified(),
    });
    if plan.whitespace_fallback {
        v["note"] = json!(
            "old_string matched only after ignoring whitespace differences; \
             new_string was re-indented to fit the file."
        );
    }
    v
}

/// Marks `file_path` as the path a call touches, for the approval policy.
fn path_hints(input: &Value) -> ToolHints {
    ToolHints::new().paths(input["file_path"].as_str())
}

/// What the approval prompt shows for an edit: the real diff, not the raw
/// arguments. `None` when the edit would fail anyway; the tool then reports
/// why after approval.
fn preview(plan: Result<EditPlan, String>) -> Option<Value> {
    let plan = plan.ok()?;
    Some(json!({ "path": plan.shown, "created": plan.before.is_none(), "diff": plan.diff }))
}

fn failed(error: impl std::fmt::Display) -> Value {
    json!({ "success": false, "error": error.to_string() })
}

fn lock(t: &SharedTracker) -> std::sync::MutexGuard<'_, FileTracker> {
    t.lock().unwrap_or_else(|p| p.into_inner())
}

/// The `read` tool. Records the read in `tracker`, which is what lets later
/// edits pass the staleness guard.
pub fn read_tool(fs: Arc<dyn ToolFs>, tracker: SharedTracker) -> ToolDefinition {
    ToolDefinition::new("read")
        .read_only(true)
        .description(
            "Read a file with line numbers. Use offset and limit for large files. \
             A file must be read before it can be edited or overwritten.",
        )
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
            let path = fs.normalize(Path::new(file_path));
            let shown = fs.relativize(&path).to_string_lossy().into_owned();
            let bytes = match fs.read(&path) {
                Ok(b) => b,
                Err(e) => return Ok(failed(format!("{shown}: {e}"))),
            };
            if let Ok(meta) = fs.metadata(&path) {
                lock(&tracker).record_read(&path, &meta, crate::tracker::content_hash(&bytes));
            }
            Ok(render_read(&shown, &bytes, offset, limit))
        })
}

/// The `write` tool: overwrite or create, creating parent directories.
pub fn write_tool(fs: Arc<dyn ToolFs>, tracker: SharedTracker) -> ToolDefinition {
    ToolDefinition::new("write")
        .description(
            "Write a file, replacing it entirely, and create parent directories. \
             An existing file must have been read first. Prefer edit for changes \
             to existing files.",
        )
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
        .destructive(true)
        .hints(path_hints)
        .preview({
            let (fs, tracker) = (Arc::clone(&fs), Arc::clone(&tracker));
            move |input| {
                let (path, content) = (input["file_path"].as_str()?, input["content"].as_str()?);
                let t = lock(&tracker);
                preview(plan_write(fs.as_ref(), &t, path, content))
            }
        })
        .handler(move |params| {
            let file_path = params["file_path"]
                .as_str()
                .ok_or("file_path must be a string")?;
            let content = params["content"]
                .as_str()
                .ok_or("content must be a string")?;
            let mut t = lock(&tracker);
            let plan = match plan_write(fs.as_ref(), &t, file_path, content) {
                Ok(p) => p,
                Err(e) => return Ok(failed(e)),
            };
            if let Err(e) = commit_plan(fs.as_ref(), &mut t, &plan) {
                return Ok(failed(e));
            }
            let verb = if plan.before.is_some() {
                "Wrote"
            } else {
                "Created"
            };
            Ok(plan_result(&plan, verb))
        })
}

/// The `edit` tool: exact-string replacement.
pub fn edit_tool(fs: Arc<dyn ToolFs>, tracker: SharedTracker) -> ToolDefinition {
    ToolDefinition::new("edit")
        .description(
            "Replace text in a file. old_string must match the file exactly and be \
             unique, unless replace_all is set. The file must have been read first. \
             An empty old_string creates a new file. If the match is off only by \
             whitespace, the edit is applied and re-indented, and says so.",
        )
        .param("file_path", ParameterType::String, true, "Path to the file")
        .param("old_string", ParameterType::String, true, "Text to replace")
        .param(
            "new_string",
            ParameterType::String,
            true,
            "Replacement text; must differ from old_string",
        )
        .push_param(
            ParameterDef::new(
                "replace_all",
                ParameterType::Boolean,
                false,
                "Replace every occurrence (default false)",
            )
            .with_default(json!(false)),
        )
        .destructive(true)
        .hints(path_hints)
        .preview({
            let (fs, tracker) = (Arc::clone(&fs), Arc::clone(&tracker));
            move |input| {
                let path = input["file_path"].as_str()?;
                let spec =
                    EditSpec::new(input["old_string"].as_str()?, input["new_string"].as_str()?)
                        .replace_all(input["replace_all"].as_bool().unwrap_or(false));
                let t = lock(&tracker);
                preview(plan_edit(fs.as_ref(), &t, path, &[spec]))
            }
        })
        .handler(move |params| {
            let file_path = params["file_path"]
                .as_str()
                .ok_or("file_path must be a string")?;
            let spec = EditSpec::new(
                params["old_string"]
                    .as_str()
                    .ok_or("old_string must be a string")?,
                params["new_string"]
                    .as_str()
                    .ok_or("new_string must be a string")?,
            )
            .replace_all(params["replace_all"].as_bool().unwrap_or(false));
            run_edits(fs.as_ref(), &tracker, file_path, &[spec])
        })
}

/// The `multi_edit` tool: several edits to one file, written once.
pub fn multi_edit_tool(fs: Arc<dyn ToolFs>, tracker: SharedTracker) -> ToolDefinition {
    ToolDefinition::new("multi_edit")
        .description(
            "Apply several edits to one file in order, writing once. If any edit \
             fails, nothing is written and the failing index is reported. Each edit \
             sees the result of the previous ones. Same matching rules as edit.",
        )
        .param("file_path", ParameterType::String, true, "Path to the file")
        .param(
            "edits",
            ParameterType::Array,
            true,
            "List of {old_string, new_string, replace_all?} objects",
        )
        .destructive(true)
        .hints(path_hints)
        .preview({
            let (fs, tracker) = (Arc::clone(&fs), Arc::clone(&tracker));
            move |input| {
                let path = input["file_path"].as_str()?;
                let edits: Vec<EditSpec> = serde_json::from_value(input["edits"].clone()).ok()?;
                let t = lock(&tracker);
                preview(plan_edit(fs.as_ref(), &t, path, &edits))
            }
        })
        .handler(move |params| {
            let file_path = params["file_path"]
                .as_str()
                .ok_or("file_path must be a string")?;
            let edits: Vec<EditSpec> = serde_json::from_value(params["edits"].clone())
                .map_err(|e| format!("invalid edits: {e}"))?;
            run_edits(fs.as_ref(), &tracker, file_path, &edits)
        })
}

fn run_edits(
    fs: &dyn ToolFs,
    tracker: &SharedTracker,
    file_path: &str,
    edits: &[EditSpec],
) -> Result<Value, String> {
    let mut t = lock(tracker);
    let plan = match plan_edit(fs, &t, file_path, edits) {
        Ok(p) => p,
        Err(e) => return Ok(failed(e)),
    };
    if let Err(e) = commit_plan(fs, &mut t, &plan) {
        return Ok(failed(e));
    }
    let verb = if plan.before.is_some() {
        "Edited"
    } else {
        "Created"
    };
    Ok(plan_result(&plan, verb))
}

/// The coding tools bound to one working tree and one tracker.
///
/// # Examples
///
/// ```rust
/// use std::sync::Arc;
/// use cosmos_llm_coding_tools::{CodingTools, DiskFs};
/// use cosmos_llm_tool::Registry;
/// use serde_json::json;
///
/// let dir = tempfile::tempdir().unwrap();
/// let tools = CodingTools::new(Arc::new(DiskFs::new(dir.path()).unwrap()));
/// let mut registry = Registry::new();
/// tools.register_all(&mut registry);
/// assert!(registry.registered("edit"));
///
/// let write = registry.get("write").unwrap();
/// write.call(&json!({ "file_path": "a.txt", "content": "hi\n" })).unwrap();
/// ```
#[derive(Clone)]
pub struct CodingTools {
    fs: Arc<DiskFs>,
    tracker: SharedTracker,
    shell: ShellTools,
}

impl CodingTools {
    /// Binds the tools to `fs` with a fresh tracker.
    pub fn new(fs: Arc<DiskFs>) -> Self {
        Self::with_tracker(fs, FileTracker::shared())
    }

    /// Binds the tools to `fs` and an existing tracker.
    pub fn with_tracker(fs: Arc<DiskFs>, tracker: SharedTracker) -> Self {
        let shell = ShellTools::new(Arc::clone(&fs), Arc::clone(&tracker));
        Self { fs, tracker, shell }
    }

    /// Replaces the shell settings (shell binary, timeouts, job cap).
    ///
    /// Resets the tracked working directory and the job table.
    #[must_use]
    pub fn with_shell_config(mut self, config: ShellConfig) -> Self {
        self.shell = ShellTools::with_config(Arc::clone(&self.fs), self.tracker(), config);
        self
    }

    /// The shell tools and their state (working directory, jobs).
    pub fn shell(&self) -> &ShellTools {
        &self.shell
    }

    /// The tracker the tools share.
    pub fn tracker(&self) -> SharedTracker {
        Arc::clone(&self.tracker)
    }

    /// The filesystem the tools run on.
    pub fn fs(&self) -> Arc<DiskFs> {
        Arc::clone(&self.fs)
    }

    fn dyn_fs(&self) -> Arc<dyn ToolFs> {
        self.fs.clone()
    }

    /// `read`, `write`, `edit`, `multi_edit`, `glob`, `grep`, `bash`,
    /// `job_output` and `job_kill`.
    ///
    /// Use [`file_tools`](Self::file_tools) to leave the shell out.
    pub fn all(&self) -> Vec<ToolDefinition> {
        let mut tools = self.file_tools();
        tools.extend(self.shell.all());
        tools
    }

    /// `read`, `write`, `edit`, `multi_edit`, `glob` and `grep`.
    pub fn file_tools(&self) -> Vec<ToolDefinition> {
        vec![
            read_tool(self.dyn_fs(), self.tracker()),
            write_tool(self.dyn_fs(), self.tracker()),
            edit_tool(self.dyn_fs(), self.tracker()),
            multi_edit_tool(self.dyn_fs(), self.tracker()),
            glob_tool(self.dyn_fs()),
            grep_tool(self.fs(), GrepBackend::Auto),
        ]
    }

    /// Registers every tool in `registry`.
    pub fn register_all(&self, registry: &mut Registry) {
        for t in self.all() {
            registry.register(t);
        }
    }
}
