# 140 — Search on disk

**Status:** implemented — `glob` (ignore-aware, newest first, capped at 100), `grep` (`rg --json` with an in-process fallback, modes, context, line cuts, head limit) and `FileIndex` (nucleo) in `cosmos-llm-coding-tools::search`; deferred: multiline grep, the `grep-searcher` crates (the fallback uses `regex` line by line), parallel walking, notify-based index refresh, tool-output spill (180), and LSP.

**Crate:** `cosmos-llm-coding-tools` · **Depends on:** 100 · **Blocks:** nothing (first milestone can use the ported presets)

## Problem

The VFS `glob` and `grep` presets iterate an in-memory tree. On a real repo
they need to:

- skip what `.gitignore`, `.ignore` and global git excludes say, plus
  `node_modules`, `target`, `.git` even without a gitignore,
- be fast on 100k-file trees,
- cap output so a broad pattern does not fill the context.

A frontend also needs fuzzy path matching for `@file` mentions in the
composer. It is not a tool, but the same walker serves it.

## Design

### Walker

`DiskFs::walk` (100) uses `ignore::WalkBuilder` with `git_ignore`,
`git_global`, `git_exclude`, `hidden(false)` plus a fixed skip list. Parallel
walk (`build_parallel`) for grep.

### `glob`

Same schema as the preset. Sort by mtime, newest first. Recent files are
usually the relevant ones, and Claude Code's `Glob` does the same. Cap at 100
results, and add "(truncated, N more; narrow the pattern)".

### `grep`

Two backends behind one tool:

1. **ripgrep subprocess** if `rg` is on `PATH`: fastest, and its options are
   what models expect (`-i`, `-n`, `-A/-B/-C`, `--type`, multiline). Parse
   `rg --json`.
2. **In-process fallback** with `grep-searcher` + `grep-regex` (the ripgrep
   library crates) over the `ignore` walker.

Params: `pattern`, `path`, `glob`, `type`, `case_insensitive`,
`output_mode: content | files_with_matches | count`, `context` lines,
`head_limit`. Default to `files_with_matches`: it is cheap, and the model
then reads what it needs.

Output limits per 180. Lines longer than ~500 chars are cut, because minified
files will otherwise drown everything else.

### Fuzzy file search (frontend support)

```rust
pub struct FileIndex { ... }
impl FileIndex {
    pub fn build(fs: &DiskFs) -> Self;          // walk once
    pub fn refresh(&mut self, fs: &DiskFs);     // re-walk; later, notify-based
    pub fn query(&self, q: &str, limit: usize) -> Vec<Match>; // with match indices for highlighting
}
```

Use `nucleo-matcher` (the matcher from helix) for scoring. Build in a
background task. Big repos take a second, and the composer must not block on
it.

### Later: LSP

Crush has LSP diagnostics after edits and symbol tools (`lsp_*.go`). This
gives large gains in edit accuracy, but it is a project of its own. Leave it
out of the first milestone.

## Tests

- Tree with a `.gitignore` excluding `build/`: glob and grep never return
  paths under it.
- `rg` absent (clear `PATH`) → fallback returns the same results as `rg` on a
  fixture tree.
- 1000 matches → capped, truncation note present.
- `FileIndex::query("srclib")` ranks `src/lib.rs` first.

## References

- `codex/codex-rs/file-search`: `ignore` + `nucleo` fuzzy search.
- `crush/internal/agent/tools/grep.go`, `rg.go`, `glob.go`.
- `opencode/packages/opencode/src/tool/grep.ts`: ripgrep wrapper.
