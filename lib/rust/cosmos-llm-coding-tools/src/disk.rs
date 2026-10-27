//! [`DiskFs`]: a [`ToolFs`] over a real working tree.
//!
//! The safety of every disk tool lives here, so path handling is strict:
//! paths are normalized lexically, the nearest existing ancestor is
//! canonicalized (resolving symlinks), and anything that lands outside the
//! allowed roots is refused with `PermissionDenied`.

use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

use cosmos_llm_tool::fs::{DirEntry, FsMeta, ToolFs, WalkOptions};

/// Directories skipped by [`ToolFs::walk`] even without a `.gitignore`.
pub const SKIP_DIRS: &[&str] = &[".git", "node_modules", "target"];

/// A [`ToolFs`] rooted at a workspace directory.
///
/// Writes are confined to the workspace plus any extra roots granted with
/// [`with_extra_root`](DiskFs::with_extra_root). Reads are confined the same
/// way unless [`allow_read_outside`](DiskFs::allow_read_outside) is set,
/// which suits agents that may read system headers or sibling repos but
/// should only write at home.
///
/// # Examples
///
/// ```rust
/// use std::path::Path;
/// use cosmos_llm_coding_tools::DiskFs;
/// use cosmos_llm_tool::fs::ToolFs;
///
/// let dir = tempfile::tempdir().unwrap();
/// let fs = DiskFs::new(dir.path()).unwrap();
/// fs.write(Path::new("a/b.txt"), b"hi").unwrap();
/// assert_eq!(fs.read(Path::new("a/b.txt")).unwrap(), b"hi");
/// assert!(fs.read(Path::new("../outside")).is_err());
/// ```
#[derive(Debug, Clone)]
pub struct DiskFs {
    root: PathBuf,
    extra_roots: Vec<PathBuf>,
    read_outside: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Access {
    Read,
    Write,
}

fn denied(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!(
            "path is outside the allowed directories: {}",
            path.display()
        ),
    )
}

/// Lexical normalization of an absolute path (`.` and `..` resolved without
/// touching the disk).
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
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

/// Canonicalizes the nearest existing ancestor of `path` and re-appends the
/// parts that do not exist yet. `path` must already be lexically normalized.
fn canonical_prefix(path: &Path) -> io::Result<PathBuf> {
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    let mut cur = path.to_path_buf();
    loop {
        match cur.canonicalize() {
            Ok(real) => {
                let mut out = real;
                for part in tail.iter().rev() {
                    out.push(part);
                }
                return Ok(out);
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let Some(name) = cur.file_name().map(|n| n.to_os_string()) else {
                    return Err(e);
                };
                tail.push(name);
                if !cur.pop() {
                    return Err(e);
                }
            }
            Err(e) => return Err(e),
        }
    }
}

impl DiskFs {
    /// Roots the filesystem at `root`, which must exist.
    pub fn new(root: impl AsRef<Path>) -> io::Result<Self> {
        Ok(Self {
            root: root.as_ref().canonicalize()?,
            extra_roots: Vec::new(),
            read_outside: false,
        })
    }

    /// Grants read and write access to another directory.
    pub fn with_extra_root(mut self, dir: impl AsRef<Path>) -> io::Result<Self> {
        self.extra_roots.push(dir.as_ref().canonicalize()?);
        Ok(self)
    }

    /// Lets reads (not writes) go anywhere on the machine.
    pub fn allow_read_outside(mut self, yes: bool) -> Self {
        self.read_outside = yes;
        self
    }

    /// The directories tools may write to: the root and the extra roots.
    pub fn allowed_roots(&self) -> impl Iterator<Item = &Path> {
        std::iter::once(self.root.as_path()).chain(self.extra_roots.iter().map(PathBuf::as_path))
    }

    fn resolve(&self, path: &Path, access: Access) -> io::Result<PathBuf> {
        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };
        let real = canonical_prefix(&lexical(&abs))?;
        let inside = self.allowed_roots().any(|r| real.starts_with(r));
        if inside || (access == Access::Read && self.read_outside) {
            Ok(real)
        } else {
            Err(denied(path))
        }
    }
}

impl ToolFs for DiskFs {
    fn root(&self) -> &Path {
        &self.root
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(self.resolve(path, Access::Read)?)
    }

    fn write(&self, path: &Path, data: &[u8]) -> io::Result<()> {
        let target = self.resolve(path, Access::Write)?;
        let dir = target
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no parent directory"))?;
        std::fs::create_dir_all(dir)?;
        // Re-resolve now that the directories exist, in case one was a
        // dangling symlink that creating it redirected.
        let target = self.resolve(&target, Access::Write)?;
        let dir = target.parent().unwrap_or(&self.root);
        if target.is_dir() {
            return Err(io::Error::other(format!(
                "is a directory: {}",
                target.display()
            )));
        }
        let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
        tmp.write_all(data)?;
        tmp.as_file().sync_data()?;
        if let Ok(meta) = std::fs::metadata(&target) {
            let _ = std::fs::set_permissions(tmp.path(), meta.permissions());
        }
        tmp.persist(&target).map_err(|e| e.error)?;
        Ok(())
    }

    fn metadata(&self, path: &Path) -> io::Result<FsMeta> {
        let m = std::fs::metadata(self.resolve(path, Access::Read)?)?;
        Ok(FsMeta::new(
            if m.is_dir() { 0 } else { m.len() },
            m.modified().ok(),
            m.is_dir(),
        ))
    }

    fn list(&self, dir: &Path) -> io::Result<Vec<DirEntry>> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(self.resolve(dir, Access::Read)?)? {
            let entry = entry?;
            let meta = entry.metadata()?;
            out.push(DirEntry::new(
                entry.file_name().to_string_lossy().into_owned(),
                meta.is_dir(),
                if meta.is_dir() { 0 } else { meta.len() },
            ));
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    fn walk(
        &self,
        dir: &Path,
        opts: &WalkOptions,
    ) -> Box<dyn Iterator<Item = io::Result<PathBuf>> + '_> {
        let start = match self.resolve(dir, Access::Read) {
            Ok(p) => p,
            Err(e) => return Box::new(std::iter::once(Err(e))),
        };
        let mut b = ignore::WalkBuilder::new(&start);
        b.hidden(false).require_git(false).follow_links(false);
        if opts.respect_ignore {
            b.git_ignore(true)
                .git_global(true)
                .git_exclude(true)
                .ignore(true)
                .parents(true)
                .filter_entry(|e| {
                    // The start directory itself may be named `target`.
                    e.depth() == 0
                        || !(e.file_type().is_some_and(|t| t.is_dir())
                            && SKIP_DIRS.iter().any(|s| e.file_name() == *s))
                });
        } else {
            b.standard_filters(false);
        }
        if let Some(d) = opts.max_depth {
            b.max_depth(Some(d + 1));
        }
        Box::new(b.build().filter_map(|r| match r {
            Ok(e) if e.file_type().is_some_and(|t| t.is_file()) => Some(Ok(e.into_path())),
            Ok(_) => None,
            Err(e) => Some(Err(io::Error::other(e.to_string()))),
        }))
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(self.resolve(path, Access::Write)?)
    }

    fn normalize(&self, path: &Path) -> PathBuf {
        self.resolve(path, Access::Read).unwrap_or_else(|_| {
            if path.is_absolute() {
                lexical(path)
            } else {
                lexical(&self.root.join(path))
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    fn fs() -> (tempfile::TempDir, DiskFs) {
        let dir = tempfile::tempdir().unwrap();
        let fs = DiskFs::new(dir.path()).unwrap();
        (dir, fs)
    }

    #[test]
    fn rejects_parent_escape_and_absolute_outside() {
        let (_d, fs) = fs();
        let e = fs.read(Path::new("../outside")).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
        let e = fs.write(Path::new("/etc/cosmos-test"), b"x").unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
        let e = fs.read(Path::new("/etc/passwd")).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
        // `..` that stays inside is fine.
        fs.write(Path::new("a/b.txt"), b"x").unwrap();
        assert!(fs.read(Path::new("a/../a/b.txt")).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_pointing_out() {
        let (d, fs) = fs();
        std::os::unix::fs::symlink("/etc", d.path().join("etc-link")).unwrap();
        let e = fs.read(Path::new("etc-link/passwd")).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
        let e = fs.write(Path::new("etc-link/new"), b"x").unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
        // A symlink to somewhere inside is fine.
        fs.write(Path::new("real.txt"), b"ok").unwrap();
        std::os::unix::fs::symlink(d.path().join("real.txt"), d.path().join("ln.txt")).unwrap();
        assert_eq!(fs.read(Path::new("ln.txt")).unwrap(), b"ok");
    }

    #[test]
    fn extra_roots_and_read_outside() {
        let (_d, fs) = fs();
        let other = tempfile::tempdir().unwrap();
        std::fs::write(other.path().join("f.txt"), "o").unwrap();
        let target = other.path().join("f.txt");
        assert!(fs.read(&target).is_err());

        let granted = fs.clone().with_extra_root(other.path()).unwrap();
        assert_eq!(granted.read(&target).unwrap(), b"o");
        granted.write(&target, b"p").unwrap();

        let ro = fs.allow_read_outside(true);
        assert_eq!(ro.read(&target).unwrap(), b"p");
        assert!(ro.write(&target, b"q").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn write_keeps_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let (d, fs) = fs();
        let p = d.path().join("run.sh");
        std::fs::write(&p, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        fs.write(Path::new("run.sh"), b"#!/bin/sh\necho\n").unwrap();
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755);
    }

    #[test]
    fn metadata_list_remove() {
        let (_d, fs) = fs();
        fs.write(Path::new("x/a.txt"), b"abc").unwrap();
        fs.write(Path::new("b.txt"), b"").unwrap();
        assert!(fs.metadata(Path::new("x")).unwrap().is_dir);
        assert_eq!(fs.metadata(Path::new("x/a.txt")).unwrap().len, 3);
        let names: Vec<_> = fs
            .list(Path::new(""))
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, ["b.txt", "x"]);
        fs.remove(Path::new("b.txt")).unwrap();
        assert!(!fs.exists(Path::new("b.txt")));
    }

    #[test]
    fn normalize_unifies_spellings() {
        let (d, fs) = fs();
        fs.write(Path::new("s/a.txt"), b"").unwrap();
        let abs = fs.normalize(&d.path().join("s/a.txt"));
        assert_eq!(abs, fs.normalize(Path::new("s/./x/../a.txt")));
        assert_eq!(fs.relativize(&abs), PathBuf::from("s/a.txt"));
    }

    #[test]
    fn walk_honors_gitignore_and_skip_list() {
        let (_d, fs) = fs();
        fs.write(Path::new(".gitignore"), b"build/\n").unwrap();
        fs.write(Path::new("build/out.o"), b"").unwrap();
        fs.write(Path::new("node_modules/m/i.js"), b"").unwrap();
        fs.write(Path::new("target/debug/x"), b"").unwrap();
        fs.write(Path::new(".git/config"), b"").unwrap();
        fs.write(Path::new("src/lib.rs"), b"").unwrap();
        fs.write(Path::new(".hidden"), b"").unwrap();
        let got = |opts: &WalkOptions| {
            let mut v: Vec<String> = fs
                .walk(Path::new(""), opts)
                .map(|r| fs.relativize(&r.unwrap()).to_string_lossy().into_owned())
                .collect();
            v.sort();
            v
        };
        assert_eq!(
            got(&WalkOptions::default()),
            [".gitignore", ".hidden", "src/lib.rs"]
        );
        let all = got(&WalkOptions::default().respect_ignore(false));
        assert!(all.contains(&"build/out.o".to_string()));
        assert!(all.contains(&".git/config".to_string()));
    }

    #[test]
    fn atomic_write_never_exposes_a_truncated_file() {
        let (_d, fs) = fs();
        let fs = Arc::new(fs);
        let a = vec![b'a'; 2_000_000];
        let b = vec![b'b'; 1_000_000];
        fs.write(Path::new("big.txt"), &a).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let reader = {
            let fs = fs.clone();
            let stop = stop.clone();
            std::thread::spawn(move || {
                let mut seen = 0;
                while !stop.load(Ordering::Relaxed) {
                    let got = fs.read(Path::new("big.txt")).unwrap();
                    assert!(
                        got.len() == 2_000_000 || got.len() == 1_000_000,
                        "{}",
                        got.len()
                    );
                    seen += 1;
                }
                seen
            })
        };
        for i in 0..40 {
            fs.write(Path::new("big.txt"), if i % 2 == 0 { &b } else { &a })
                .unwrap();
        }
        stop.store(true, Ordering::Relaxed);
        assert!(reader.join().unwrap() > 0);
    }
}
