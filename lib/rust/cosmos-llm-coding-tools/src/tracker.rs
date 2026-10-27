//! File tracking: staleness checks, per-turn change records, turn diffs and
//! undo.
//!
//! One [`FileTracker`] per thread, shared by the tools as a
//! [`SharedTracker`]. It records what the model read (to refuse edits to
//! files it has not seen, or that changed under it) and what the agent wrote
//! (to show a combined diff and to revert a turn without involving git).

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use cosmos_llm_tool::fs::{FsMeta, ToolFs};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::diff::{file_diff, ChangeKind, FileDiff};

/// A tracker shared between tools.
pub type SharedTracker = Arc<Mutex<FileTracker>>;

/// SHA-256 of `bytes`, as lowercase hex. Used for read stamps and blob ids.
pub fn content_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// What the model saw the last time it read a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadStamp {
    /// Length in bytes.
    pub len: u64,
    /// Modification time, when the backend has one.
    pub mtime: Option<SystemTime>,
    /// [`content_hash`] of the content.
    pub hash: String,
    /// `false` after something that may have changed files behind the
    /// tracker's back (a shell command); the next check compares hashes.
    pub verified: bool,
}

/// Why a file failed [`FileTracker::check_fresh`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Stale {
    /// The file was never read in this session.
    #[error("File has not been read yet. Read it first before editing.")]
    NeverRead,
    /// The file changed on disk since it was read.
    #[error("File has been modified since it was last read. Read it again before editing.")]
    Modified,
    /// The file was read but no longer exists.
    #[error("File no longer exists. Read the directory again before editing.")]
    Missing,
}

/// A file touched during a turn. Blob ids refer to
/// [`FileTracker::blob`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TurnFile {
    /// Normalized path of the file.
    pub path: String,
    /// Blob id of the content before the turn's first write; absent if the
    /// file did not exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
    /// [`content_hash`] after the turn's last write; absent if the file was
    /// deleted. Revert compares it with the current content to detect later
    /// changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

/// The files one turn changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TurnChanges {
    /// Turn number.
    pub turn: u32,
    /// Files in the order first touched.
    #[serde(default)]
    pub files: Vec<TurnFile>,
}

/// The outcome of [`FileTracker::revert_turn`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RevertReport {
    /// Paths restored (or deleted, if the turn created them).
    #[serde(default)]
    pub reverted: Vec<String>,
    /// Paths left alone because they changed after the turn.
    #[serde(default)]
    pub skipped: Vec<String>,
}

/// Why a revert failed outright.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RevertError {
    /// No changes are recorded for that turn.
    #[error("no recorded changes for turn {0}")]
    UnknownTurn(u32),
    /// A blob needed to restore a file is missing from the tracker.
    #[error("missing saved content for {0}")]
    MissingBlob(String),
    /// Restoring a file failed.
    #[error("could not restore {path}: {source}")]
    Io {
        /// The file that failed.
        path: String,
        /// The underlying error.
        source: io::Error,
    },
}

/// Records reads and writes. See the [module docs](self).
///
/// # Examples
///
/// ```rust
/// use std::path::Path;
/// use cosmos_llm_coding_tools::tracker::{content_hash, FileTracker};
/// use cosmos_llm_tool::fs::{ToolFs, VfsFs};
///
/// let fs = VfsFs::empty();
/// fs.write(Path::new("a.txt"), b"one").unwrap();
/// let mut t = FileTracker::new();
/// let meta = fs.metadata(Path::new("a.txt")).unwrap();
/// t.record_read(Path::new("a.txt"), &meta, content_hash(b"one"));
/// assert!(t.check_fresh(&fs, Path::new("a.txt")).is_ok());
/// fs.write(Path::new("a.txt"), b"two").unwrap();
/// assert!(t.check_fresh(&fs, Path::new("a.txt")).is_err());
/// ```
#[derive(Debug, Default)]
pub struct FileTracker {
    reads: HashMap<PathBuf, ReadStamp>,
    turns: Vec<TurnChanges>,
    blobs: HashMap<String, Vec<u8>>,
    turn: u32,
}

impl FileTracker {
    /// An empty tracker at turn 0.
    pub fn new() -> Self {
        Self::default()
    }

    /// A tracker ready to share between tools.
    pub fn shared() -> SharedTracker {
        Arc::new(Mutex::new(Self::new()))
    }

    /// The current turn number.
    pub fn turn(&self) -> u32 {
        self.turn
    }

    /// Starts the next turn and returns its number.
    pub fn begin_turn(&mut self) -> u32 {
        self.turn += 1;
        self.turn
    }

    /// Notes that the model has seen `path` with this content.
    ///
    /// `path` is a [`ToolFs::normalize`]d path.
    pub fn record_read(&mut self, path: &Path, meta: &FsMeta, hash: String) {
        self.reads.insert(
            path.to_path_buf(),
            ReadStamp {
                len: meta.len,
                mtime: meta.mtime,
                hash,
                verified: true,
            },
        );
    }

    /// Checks that `path` is unchanged since it was read.
    ///
    /// Length and mtime are compared first, and the content is hashed only
    /// when they differ (or the stamp is unverified): editors sometimes
    /// rewrite a file without changing it.
    pub fn check_fresh(&self, fs: &dyn ToolFs, path: &Path) -> Result<(), Stale> {
        let stamp = self.reads.get(path).ok_or(Stale::NeverRead)?;
        let meta = fs.metadata(path).map_err(|_| Stale::Missing)?;
        if stamp.verified
            && meta.len == stamp.len
            && meta.mtime.is_some()
            && meta.mtime == stamp.mtime
        {
            return Ok(());
        }
        let bytes = fs.read(path).map_err(|_| Stale::Missing)?;
        if content_hash(&bytes) == stamp.hash {
            Ok(())
        } else {
            Err(Stale::Modified)
        }
    }

    /// Marks every stamp unverified, so the next check compares hashes.
    ///
    /// Call after anything that can change files without the tracker
    /// knowing, such as a shell command.
    pub fn mark_all_unverified(&mut self) {
        for s in self.reads.values_mut() {
            s.verified = false;
        }
    }

    /// Records a write by the agent in `turn`.
    ///
    /// `before` is the content before this write (`None` if the file did not
    /// exist); it is kept only for the first write to the file in the turn.
    /// `after` is the content now (`None` if deleted). The read stamp is
    /// refreshed, so a write followed by an edit does not trip the guard.
    pub fn record_write(
        &mut self,
        fs: &dyn ToolFs,
        turn: u32,
        path: &Path,
        before: Option<Vec<u8>>,
        after: Option<&[u8]>,
    ) {
        let key = path.to_string_lossy().into_owned();
        let after_hash = after.map(content_hash);
        let idx = match self.turns.iter().position(|t| t.turn == turn) {
            Some(i) => i,
            None => {
                self.turns.push(TurnChanges {
                    turn,
                    files: Vec::new(),
                });
                self.turns.len() - 1
            }
        };
        let known = self.turns[idx].files.iter().position(|f| f.path == key);
        match known {
            Some(i) => self.turns[idx].files[i].after = after_hash.clone(),
            None => {
                let before_id = before.map(|b| {
                    let id = content_hash(&b);
                    self.blobs.entry(id.clone()).or_insert(b);
                    id
                });
                self.turns[idx].files.push(TurnFile {
                    path: key,
                    before: before_id,
                    after: after_hash.clone(),
                });
            }
        }
        match (after, fs.metadata(path)) {
            (Some(_), Ok(meta)) => self.record_read(path, &meta, after_hash.unwrap_or_default()),
            _ => {
                self.reads.remove(path);
            }
        }
    }

    /// The changes recorded for `turn`.
    pub fn changes(&self, turn: u32) -> Option<&TurnChanges> {
        self.turns.iter().find(|t| t.turn == turn)
    }

    /// Every turn's changes, oldest first.
    pub fn all_changes(&self) -> &[TurnChanges] {
        &self.turns
    }

    /// Saved content for a blob id from [`TurnFile::before`].
    pub fn blob(&self, id: &str) -> Option<&[u8]> {
        self.blobs.get(id).map(Vec::as_slice)
    }

    /// Combined diff of everything `turn` changed, against what is on disk
    /// now.
    pub fn turn_diff(&self, fs: &dyn ToolFs, turn: u32) -> Vec<FileDiff> {
        let Some(changes) = self.changes(turn) else {
            return Vec::new();
        };
        changes
            .files
            .iter()
            .map(|f| {
                let path = Path::new(&f.path);
                let shown = fs.relativize(path).to_string_lossy().into_owned();
                let before = f.before.as_deref().and_then(|id| self.blob(id));
                let now = fs.read(path).ok();
                diff_bytes(&shown, before, now.as_deref())
            })
            .collect()
    }

    /// Restores every file `turn` changed to its content before the turn,
    /// deleting files the turn created.
    ///
    /// A file that changed after the turn (by a later turn or by the user)
    /// is skipped and reported, unless `force` is set. Restored files are
    /// dropped from the turn's record, so reverting twice is harmless.
    /// Side effects of shell commands are not tracked and cannot be undone.
    pub fn revert_turn(
        &mut self,
        fs: &dyn ToolFs,
        turn: u32,
        force: bool,
    ) -> Result<RevertReport, RevertError> {
        let idx = self
            .turns
            .iter()
            .position(|t| t.turn == turn)
            .ok_or(RevertError::UnknownTurn(turn))?;
        let files = self.turns[idx].files.clone();
        let mut report = RevertReport::default();
        let mut done: Vec<String> = Vec::new();
        for f in &files {
            let path = Path::new(&f.path);
            let shown = fs.relativize(path).to_string_lossy().into_owned();
            let current = fs.read(path).ok();
            let current_hash = current.as_deref().map(content_hash);
            if !force && current_hash != f.after {
                report.skipped.push(shown);
                continue;
            }
            let io_err = |source| RevertError::Io {
                path: shown.clone(),
                source,
            };
            match &f.before {
                Some(id) => {
                    let data = self
                        .blobs
                        .get(id)
                        .ok_or_else(|| RevertError::MissingBlob(shown.clone()))?;
                    fs.write(path, data).map_err(io_err)?;
                }
                None => {
                    if current.is_some() {
                        fs.remove(path).map_err(io_err)?;
                    }
                }
            }
            self.reads.remove(path);
            done.push(f.path.clone());
            report.reverted.push(shown);
        }
        self.turns[idx].files.retain(|f| !done.contains(&f.path));
        Ok(report)
    }
}

fn diff_bytes(shown: &str, before: Option<&[u8]>, after: Option<&[u8]>) -> FileDiff {
    let text = |b: Option<&[u8]>| -> Result<Option<String>, ()> {
        match b {
            None => Ok(None),
            Some(b) => String::from_utf8(b.to_vec()).map(Some).map_err(|_| ()),
        }
    };
    match (text(before), text(after)) {
        (Ok(b), Ok(a)) => file_diff(shown, b.as_deref(), a.as_deref()),
        _ => {
            let kind = match (before, after) {
                (None, _) => ChangeKind::Created,
                (_, None) => ChangeKind::Deleted,
                _ => ChangeKind::Modified,
            };
            FileDiff::binary(shown, kind)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmos_llm_tool::fs::VfsFs;

    fn read(t: &mut FileTracker, fs: &VfsFs, p: &str) {
        let path = Path::new(p);
        let bytes = fs.read(path).unwrap();
        t.record_read(path, &fs.metadata(path).unwrap(), content_hash(&bytes));
    }

    fn agent_write(t: &mut FileTracker, fs: &VfsFs, p: &str, data: &[u8]) {
        let path = Path::new(p);
        let before = fs.read(path).ok();
        fs.write(path, data).unwrap();
        let turn = t.turn();
        t.record_write(fs, turn, path, before, Some(data));
    }

    #[test]
    fn never_read_modified_and_same_content_rewrite() {
        let fs = VfsFs::empty();
        fs.write(Path::new("a"), b"one").unwrap();
        let mut t = FileTracker::new();
        assert_eq!(t.check_fresh(&fs, Path::new("a")), Err(Stale::NeverRead));
        read(&mut t, &fs, "a");
        assert!(t.check_fresh(&fs, Path::new("a")).is_ok());
        // Same bytes, new mtime: still fresh.
        fs.write(Path::new("a"), b"one").unwrap();
        assert!(t.check_fresh(&fs, Path::new("a")).is_ok());
        fs.write(Path::new("a"), b"two").unwrap();
        assert_eq!(t.check_fresh(&fs, Path::new("a")), Err(Stale::Modified));
        fs.remove(Path::new("a")).unwrap();
        assert_eq!(t.check_fresh(&fs, Path::new("a")), Err(Stale::Missing));
    }

    #[test]
    fn agent_write_keeps_file_fresh() {
        let fs = VfsFs::empty();
        fs.write(Path::new("a"), b"one").unwrap();
        let mut t = FileTracker::new();
        read(&mut t, &fs, "a");
        agent_write(&mut t, &fs, "a", b"two");
        assert!(t.check_fresh(&fs, Path::new("a")).is_ok());
    }

    #[test]
    fn unverified_stamps_force_a_hash_check() {
        let fs = VfsFs::empty();
        fs.write(Path::new("a"), b"one").unwrap();
        let mut t = FileTracker::new();
        read(&mut t, &fs, "a");
        t.mark_all_unverified();
        assert!(t.check_fresh(&fs, Path::new("a")).is_ok());
    }

    #[test]
    fn two_writes_in_a_turn_keep_the_first_before() {
        let fs = VfsFs::empty();
        fs.write(Path::new("a"), b"v0").unwrap();
        let mut t = FileTracker::new();
        t.begin_turn();
        agent_write(&mut t, &fs, "a", b"v1");
        agent_write(&mut t, &fs, "a", b"v2");
        let c = t.changes(1).unwrap();
        assert_eq!(c.files.len(), 1);
        let id = c.files[0].before.as_deref().unwrap();
        assert_eq!(t.blob(id), Some(&b"v0"[..]));
        assert_eq!(c.files[0].after, Some(content_hash(b"v2")));
        let d = t.turn_diff(&fs, 1);
        assert_eq!(d.len(), 1);
        assert_eq!((d[0].added, d[0].removed), (1, 1));
    }

    #[test]
    fn revert_restores_and_deletes_created() {
        let fs = VfsFs::empty();
        fs.write(Path::new("old"), b"keep").unwrap();
        let mut t = FileTracker::new();
        t.begin_turn();
        agent_write(&mut t, &fs, "old", b"changed");
        agent_write(&mut t, &fs, "new", b"fresh");
        let r = t.revert_turn(&fs, 1, false).unwrap();
        assert_eq!(r.reverted.len(), 2);
        assert!(r.skipped.is_empty());
        assert_eq!(fs.read(Path::new("old")).unwrap(), b"keep");
        assert!(!fs.exists(Path::new("new")));
        // Second revert has nothing to do.
        assert!(t.revert_turn(&fs, 1, false).unwrap().reverted.is_empty());
        assert!(matches!(
            t.revert_turn(&fs, 9, false),
            Err(RevertError::UnknownTurn(9))
        ));
    }

    #[test]
    fn revert_skips_later_changes_unless_forced() {
        let fs = VfsFs::empty();
        fs.write(Path::new("a"), b"v0").unwrap();
        fs.write(Path::new("b"), b"b0").unwrap();
        let mut t = FileTracker::new();
        t.begin_turn();
        agent_write(&mut t, &fs, "a", b"v1");
        agent_write(&mut t, &fs, "b", b"b1");
        // The user edits `a` afterwards.
        fs.write(Path::new("a"), b"user").unwrap();
        let r = t.revert_turn(&fs, 1, false).unwrap();
        assert_eq!(r.skipped, ["a"]);
        assert_eq!(r.reverted, ["b"]);
        assert_eq!(fs.read(Path::new("a")).unwrap(), b"user");
        let r = t.revert_turn(&fs, 1, true).unwrap();
        assert_eq!(r.reverted, ["a"]);
        assert_eq!(fs.read(Path::new("a")).unwrap(), b"v0");
    }

    #[test]
    fn later_turn_blocks_earlier_revert() {
        let fs = VfsFs::empty();
        fs.write(Path::new("a"), b"v0").unwrap();
        let mut t = FileTracker::new();
        t.begin_turn();
        agent_write(&mut t, &fs, "a", b"v1");
        t.begin_turn();
        agent_write(&mut t, &fs, "a", b"v2");
        let r = t.revert_turn(&fs, 1, false).unwrap();
        assert_eq!(r.skipped, ["a"]);
        let r = t.revert_turn(&fs, 2, false).unwrap();
        assert_eq!(r.reverted, ["a"]);
        assert_eq!(fs.read(Path::new("a")).unwrap(), b"v1");
    }
}
