//! A filesystem trait for tools, so one set of presets runs on the in-memory
//! VFS and on a real working tree.
//!
//! [`ToolFs`] is deliberately small and synchronous. Local file I/O is fast,
//! and a sync trait stays usable from sync handlers; callers doing a lot of it
//! wrap calls in `spawn_blocking`.
//!
//! [`VfsFs`] is the in-memory implementation. The disk implementation,
//! `DiskFs`, lives in the `cosmos-llm-coding-tools` crate.

use std::collections::HashMap;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::RwLock;
use std::time::SystemTime;

use cosmos_llm_virtual_filesystem::Filesystem;

/// What [`ToolFs::metadata`] reports about a path.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FsMeta {
    /// Size in bytes (0 for directories).
    pub len: u64,
    /// Last modification time, when the backend tracks one.
    pub mtime: Option<SystemTime>,
    /// `true` for directories.
    pub is_dir: bool,
}

impl FsMeta {
    /// Builds a metadata record.
    pub fn new(len: u64, mtime: Option<SystemTime>, is_dir: bool) -> Self {
        Self { len, mtime, is_dir }
    }
}

/// One entry of [`ToolFs::list`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DirEntry {
    /// File or directory name (no separators).
    pub name: String,
    /// `true` for directories.
    pub is_dir: bool,
    /// Size in bytes (0 for directories).
    pub len: u64,
}

impl DirEntry {
    /// Builds a directory entry.
    pub fn new(name: impl Into<String>, is_dir: bool, len: u64) -> Self {
        Self {
            name: name.into(),
            is_dir,
            len,
        }
    }
}

/// Options for [`ToolFs::walk`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct WalkOptions {
    /// Honor `.gitignore`, `.ignore`, global git excludes and the built-in
    /// skip list (`.git`, `node_modules`, `target`). Default `true`.
    pub respect_ignore: bool,
    /// Descend at most this many levels below the start directory.
    pub max_depth: Option<usize>,
}

impl Default for WalkOptions {
    fn default() -> Self {
        Self {
            respect_ignore: true,
            max_depth: None,
        }
    }
}

impl WalkOptions {
    /// Sets [`respect_ignore`](Self::respect_ignore).
    pub fn respect_ignore(mut self, yes: bool) -> Self {
        self.respect_ignore = yes;
        self
    }

    /// Sets [`max_depth`](Self::max_depth).
    pub fn max_depth(mut self, depth: usize) -> Self {
        self.max_depth = Some(depth);
        self
    }
}

/// The filesystem a tool operates on.
///
/// Paths are absolute or relative to [`root`](ToolFs::root). Implementations
/// decide what is reachable and return [`io::ErrorKind::PermissionDenied`] for
/// anything outside it.
pub trait ToolFs: Send + Sync {
    /// The workspace root that relative paths resolve against.
    fn root(&self) -> &Path;
    /// Reads a whole file.
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
    /// Creates or replaces a file, creating missing parent directories.
    fn write(&self, path: &Path, data: &[u8]) -> io::Result<()>;
    /// Size, modification time and kind of a path.
    fn metadata(&self, path: &Path) -> io::Result<FsMeta>;
    /// The immediate children of a directory, sorted by name.
    fn list(&self, dir: &Path) -> io::Result<Vec<DirEntry>>;
    /// Recursively lists the files under `dir` (not directories).
    ///
    /// Yielded paths are absolute for disk backends and root-relative for the
    /// VFS; pass them to [`ToolFs::relativize`] for display.
    fn walk(
        &self,
        dir: &Path,
        opts: &WalkOptions,
    ) -> Box<dyn Iterator<Item = io::Result<PathBuf>> + '_>;
    /// Deletes a file.
    fn remove(&self, path: &Path) -> io::Result<()>;

    /// A path as the model should see it: relative to the root when possible.
    fn relativize(&self, path: &Path) -> PathBuf {
        path.strip_prefix(self.root())
            .map(Path::to_path_buf)
            .unwrap_or_else(|_| path.to_path_buf())
    }

    /// The key under which `path` is tracked: absolute (disk) or root-relative
    /// (VFS), lexically normalized, so `a/../b`, `./b` and `<root>/b` agree.
    ///
    /// Disk backends override this to resolve symlinks.
    fn normalize(&self, path: &Path) -> PathBuf {
        let joined = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root().join(path)
        };
        let mut out = PathBuf::new();
        for c in joined.components() {
            match c {
                Component::ParentDir => {
                    out.pop();
                }
                Component::CurDir => {}
                other => out.push(other.as_os_str()),
            }
        }
        out
    }

    /// `true` if `path` exists.
    fn exists(&self, path: &Path) -> bool {
        self.metadata(path).is_ok()
    }
}

/// Lexically normalizes a path into `/`-separated parts. `..` past the start
/// is an error. A leading root component is dropped.
fn vfs_parts(path: &Path) -> io::Result<Vec<String>> {
    let mut parts: Vec<String> = Vec::new();
    for c in path.components() {
        match c {
            Component::Prefix(_) | Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                if parts.pop().is_none() {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        format!("path escapes the filesystem root: {}", path.display()),
                    ));
                }
            }
            Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
        }
    }
    Ok(parts)
}

fn not_found(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("not found: {}", path.display()),
    )
}

struct VfsState {
    fs: Filesystem,
    /// Write times of files changed since construction. Files from the
    /// initial tree report the epoch.
    mtimes: HashMap<String, SystemTime>,
}

/// [`ToolFs`] over the in-memory [`Filesystem`].
///
/// The tree sits behind an `RwLock`, so `write` can replace a file in place
/// even though `Filesystem` itself is built once.
///
/// # Examples
///
/// ```rust
/// use std::path::Path;
/// use cosmos_llm_tool::fs::{ToolFs, VfsFs};
///
/// let fs = VfsFs::empty();
/// fs.write(Path::new("a/b.txt"), b"hi").unwrap();
/// assert_eq!(fs.read(Path::new("a/b.txt")).unwrap(), b"hi");
/// ```
pub struct VfsFs {
    state: RwLock<VfsState>,
}

impl VfsFs {
    /// Wraps an existing tree.
    pub fn new(fs: Filesystem) -> Self {
        Self {
            state: RwLock::new(VfsState {
                fs,
                mtimes: HashMap::new(),
            }),
        }
    }

    /// An empty tree rooted at `/`.
    pub fn empty() -> Self {
        Self::new(Filesystem::new("/"))
    }

    /// A copy of the current tree.
    pub fn snapshot(&self) -> Filesystem {
        self.state.read().unwrap().fs.clone()
    }
}

impl ToolFs for VfsFs {
    fn root(&self) -> &Path {
        Path::new("")
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        let key = vfs_parts(path)?.join("/");
        let st = self.state.read().unwrap();
        let f = st.fs.find_file(&key).ok_or_else(|| not_found(path))?;
        Ok(f.content.clone().unwrap_or_default().into_bytes())
    }

    fn write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        let key = vfs_parts(path)?.join("/");
        let text = String::from_utf8(data.to_vec()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "the virtual filesystem holds text only",
            )
        })?;
        let mut st = self.state.write().unwrap();
        st.fs
            .set_file(&key, text)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))?;
        st.mtimes.insert(key, SystemTime::now());
        Ok(())
    }

    fn metadata(&self, path: &Path) -> io::Result<FsMeta> {
        let key = vfs_parts(path)?.join("/");
        let st = self.state.read().unwrap();
        if let Some(f) = st.fs.find_file(&key) {
            let len = f.content.as_ref().map_or(0, |c| c.len()) as u64;
            let mtime = st
                .mtimes
                .get(&key)
                .copied()
                .unwrap_or(SystemTime::UNIX_EPOCH);
            return Ok(FsMeta::new(len, Some(mtime), false));
        }
        if st.fs.find_dir(&key).is_some() {
            return Ok(FsMeta::new(0, None, true));
        }
        Err(not_found(path))
    }

    fn list(&self, dir: &Path) -> io::Result<Vec<DirEntry>> {
        let key = vfs_parts(dir)?.join("/");
        let st = self.state.read().unwrap();
        let node = st.fs.find_dir(&key).ok_or_else(|| not_found(dir))?;
        let mut out: Vec<DirEntry> = node
            .children
            .iter()
            .map(|c| DirEntry::new(c.name.clone(), true, 0))
            .chain(node.files.iter().map(|f| {
                DirEntry::new(
                    f.name.clone(),
                    false,
                    f.content.as_ref().map_or(0, |c| c.len()) as u64,
                )
            }))
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    fn walk(
        &self,
        dir: &Path,
        opts: &WalkOptions,
    ) -> Box<dyn Iterator<Item = io::Result<PathBuf>> + '_> {
        let key = match vfs_parts(dir) {
            Ok(parts) => parts.join("/"),
            Err(e) => return Box::new(std::iter::once(Err(e))),
        };
        let st = self.state.read().unwrap();
        let Some(node) = st.fs.find_dir(&key) else {
            return Box::new(std::iter::once(Err(not_found(dir))));
        };
        let depth_limit = opts.max_depth;
        let mut out: Vec<PathBuf> = Vec::new();
        fn visit(
            node: &Filesystem,
            prefix: &Path,
            depth: usize,
            limit: Option<usize>,
            out: &mut Vec<PathBuf>,
        ) {
            for f in &node.files {
                out.push(prefix.join(&f.name));
            }
            if limit.is_some_and(|l| depth + 1 > l) {
                return;
            }
            for c in &node.children {
                visit(c, &prefix.join(&c.name), depth + 1, limit, out);
            }
        }
        let prefix: PathBuf = key.split('/').filter(|s| !s.is_empty()).collect();
        visit(node, &prefix, 0, depth_limit, &mut out);
        out.sort();
        Box::new(out.into_iter().map(Ok))
    }

    fn normalize(&self, path: &Path) -> PathBuf {
        vfs_parts(path).map_or_else(|_| path.to_path_buf(), |p| p.iter().collect())
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        let key = vfs_parts(path)?.join("/");
        let mut st = self.state.write().unwrap();
        if st.fs.remove_file(&key) {
            st.mtimes.remove(&key);
            Ok(())
        } else {
            Err(not_found(path))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> VfsFs {
        VfsFs::new(Filesystem::build("/", |r| {
            r.file("a.txt", Some("A"), Default::default()).unwrap();
            r.directory("src", |s| {
                s.file("lib.rs", Some("fn x(){}"), Default::default())
                    .unwrap();
                s.directory("deep", |d| {
                    d.file("x.rs", Some(""), Default::default()).unwrap();
                });
            });
        }))
    }

    #[test]
    fn read_write_remove() {
        let fs = sample();
        assert_eq!(fs.read(Path::new("/src/lib.rs")).unwrap(), b"fn x(){}");
        fs.write(Path::new("src/new/n.txt"), b"n").unwrap();
        assert_eq!(fs.read(Path::new("./src/new/n.txt")).unwrap(), b"n");
        fs.remove(Path::new("src/new/n.txt")).unwrap();
        assert!(fs.read(Path::new("src/new/n.txt")).is_err());
        assert!(fs.remove(Path::new("src/new/n.txt")).is_err());
    }

    #[test]
    fn normalize_unifies_spellings() {
        let fs = sample();
        let a = fs.normalize(Path::new("/src/lib.rs"));
        assert_eq!(a, fs.normalize(Path::new("./src/x/../lib.rs")));
        assert_eq!(a, PathBuf::from("src/lib.rs"));
    }

    #[test]
    fn dotdot_cannot_escape() {
        let fs = sample();
        let e = fs.read(Path::new("../x")).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
        assert!(fs.read(Path::new("src/../a.txt")).is_ok());
    }

    #[test]
    fn metadata_and_list() {
        let fs = sample();
        assert!(fs.metadata(Path::new("src")).unwrap().is_dir);
        let m = fs.metadata(Path::new("a.txt")).unwrap();
        assert_eq!(m.len, 1);
        let before = m.mtime;
        fs.write(Path::new("a.txt"), b"AB").unwrap();
        assert_ne!(fs.metadata(Path::new("a.txt")).unwrap().mtime, before);
        let names: Vec<String> = fs
            .list(Path::new(""))
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, vec!["a.txt", "src"]);
    }

    #[test]
    fn walk_lists_files_and_honors_depth() {
        let fs = sample();
        let all: Vec<_> = fs
            .walk(Path::new(""), &WalkOptions::default())
            .map(Result::unwrap)
            .collect();
        assert_eq!(all.len(), 3);
        let shallow: Vec<_> = fs
            .walk(Path::new(""), &WalkOptions::default().max_depth(0))
            .map(Result::unwrap)
            .collect();
        assert_eq!(shallow, vec![PathBuf::from("a.txt")]);
        let sub: Vec<_> = fs
            .walk(Path::new("src"), &WalkOptions::default())
            .map(Result::unwrap)
            .collect();
        assert!(sub.contains(&PathBuf::from("src/deep/x.rs")));
    }
}
