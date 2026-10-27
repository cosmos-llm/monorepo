# 100 — A filesystem trait for tools

**Status:** implemented — `ToolFs`/`VfsFs`/`FsMeta`/`DirEntry`/`WalkOptions` in `cosmos-llm-tool` (`fs` module, plus `*_tool_fs` presets over the trait and a mutating `set_file`/`remove_file` on the VFS), and `DiskFs` in `cosmos-llm-coding-tools` with root confinement, symlink checks, extra roots, optional outside reads and atomic writes; deferred: the old `Arc<Filesystem>` presets were left as they are rather than rewritten as wrappers, tools run the sync trait inline (no `spawn_blocking`), and the `read` tool does not yet return image blocks (010).

**Crate:** `cosmos-llm-tool` (trait), `cosmos-llm-coding-tools` (disk impl) · **Depends on:** nothing · **Blocks:** 110, 130, 140

## Problem

The presets (`cosmos-llm-tool/src/preset/`) take `Arc<Filesystem>`, the
in-memory VFS. That is the right sandbox for research runs, but a coding
agent edits a real working tree. Writing disk versions of `read`, `glob`,
`grep`, `list`, `write` next to the VFS ones would duplicate the parameter
schemas, descriptions and output formatting, and the two sets would drift
apart.

The VFS is also immutable once built (`Filesystem::build`; `VirtualFile` uses
`with_content` to derive copies), so the current `write` preset cannot change
anything in place.

## Design

```rust
pub trait ToolFs: Send + Sync {
    fn root(&self) -> &Path;
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
    fn write(&self, path: &Path, data: &[u8]) -> io::Result<()>;
    fn metadata(&self, path: &Path) -> io::Result<FsMeta>; // len, mtime, is_dir
    fn list(&self, dir: &Path) -> io::Result<Vec<DirEntry>>;
    /// Ignore-aware walk (140). The VFS has nothing to ignore.
    fn walk(&self, dir: &Path, opts: &WalkOptions) -> Box<dyn Iterator<Item = io::Result<PathBuf>> + '_>;
    fn remove(&self, path: &Path) -> io::Result<()>;
}
```

Sync, on purpose. File I/O on a local disk is fast, and the trait stays
usable from sync handlers. Disk tools call it inside `spawn_blocking`.

Implementations:

- `VfsFs` in `cosmos-llm-tool`, wrapping the VFS behind an `RwLock` so `write`
  can replace a file. Needs a `Filesystem::set_file` / `remove` on the VFS
  crate, the first mutating API it would have.
- `DiskFs` in `cosmos-llm-coding-tools`, rooted at the workspace.

The presets change from `read_tool(Arc<Filesystem>)` to
`read_tool(Arc<dyn ToolFs>)`. Keep the old signatures as one-line wrappers.

### Path handling (DiskFs)

This is where the safety actually lives, so it has to be strict:

- Accept absolute or root-relative paths. Normalize lexically (`..`, `.`),
  then canonicalize the nearest existing ancestor, and reject anything that
  resolves outside the allowed roots. Symlinks pointing out of the tree
  count as outside.
- Allowed roots: the workspace plus any extra directories the user granted
  (codex `additional_dirs`). Policy (150) decides what is granted; the fs
  enforces it.
- Reads outside the roots: configurable. Coding agents commonly allow reading
  anywhere and writing only inside. Make it a `DiskFs` option.
- Writes are atomic: write a temp file in the same directory, then rename.
  Keep the existing file's permissions.

### Text vs binary

`read` returns bytes. The `read` tool decides: valid UTF-8 → numbered lines
(as today); known image types → an image block (010); anything else → "binary
file, N bytes". Line endings: detect CRLF on read and keep it on write (110).

## Tests

- Same preset test suite runs against `VfsFs` and a `DiskFs` over a
  `tempfile::TempDir`. Use a macro or a generic test fn so both always run.
- `DiskFs` rejects `../outside`, an absolute path outside the root, and a
  symlink to `/etc`.
- Atomic write: a reader never sees a truncated file (write a large file in a
  loop while reading).

## References

- `codex/codex-rs/file-system`: fs abstraction.
- `crush/internal/fsext`: path helpers, ignore handling.
- `goose/crates/goose-mcp` developer tools: path resolution.
