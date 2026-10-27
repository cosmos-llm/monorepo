# 110 — Edit tools

**Status:** implemented — `read`, `write`, `edit`, `multi_edit` in `cosmos-llm-coding-tools` (`tools`, `edit` modules) with exact match, whitespace fallback with re-indent, CRLF preservation, read-before-write guard, and pure `plan_edit`/`plan_write`/`commit_plan` for previews; deferred: registering a `preview` with the approval flow (080) and the model/UI result split via `ToolOutcome::display` (060, so the unified diff currently travels in the tool result), and `apply_patch`.

**Crate:** `cosmos-llm-coding-tools` · **Depends on:** 100, 130 · **Blocks:** first milestone

## Problem

Models change code through edits. The only mutating preset is `write`, which
replaces whole files. That wastes output tokens on large files, and the model
regularly drops code it meant to keep.

## Design

Ship the edit format current models are trained on: exact string replacement.
Claude Code, crush, opencode, gemini-cli and qwen-code all use this shape, so
models use it reliably.

### `edit`

| Param | Type | |
|---|---|---|
| `file_path` | string | required |
| `old_string` | string | required; `""` with a nonexistent file creates it |
| `new_string` | string | required; must differ from `old_string` |
| `replace_all` | bool | default false |

Algorithm:

1. Guard: the file must have been read in this session and not changed on
   disk since (130). Otherwise fail with "File has been modified since it was
   last read. Read it again before editing."
2. Normalize line endings for matching. Remember whether the file uses CRLF
   and restore it on write.
3. Count exact occurrences of `old_string`.
   - 0: try whitespace-normalized matching (below). Still nothing → error
     that quotes a few nearby lines, so the model can correct itself.
   - >1 and not `replace_all`: error "found N matches; add surrounding context
     or set replace_all".
4. Replace, write atomically (100), record the write (130).
5. Result to the model: short, e.g. "Edited src/lib.rs (+3 −1)". Result to the
   UI (`ToolOutcome::display`, 060): unified diff with line numbers.

**Whitespace fallback.** Crush's `edit_whitespace.go` collapses runs of
whitespace on each line, finds whole-line matches, replaces at line
granularity, and re-indents `new_string` to the matched block's indentation.
It reports when it did this. Port it: it saves a round trip whenever the model
gets tabs vs spaces wrong. Accept a match only if it is unique.

### `multi_edit`

`file_path` + `edits: [{old_string, new_string, replace_all}]`. Edits apply in
order to the in-memory content, and the file is written once. If any edit
fails, nothing is written, and the error names the failing edit's index. This
saves turns on refactors.

### `write`

Full overwrite or create. Same read-before-write guard for existing files.
New files skip it. Creates parent directories.

### Approval preview

All three register a `preview` (080) that computes the diff without writing,
so the user approves the actual change. The preview and the execution must
compute the same result. Run the same pure function twice and guard against
the file changing in between with the 130 staleness check.

### Not now: `apply_patch`

Codex's OpenAI models are trained on the `*** Begin Patch` format
(`codex-rs/apply-patch`). Add it later as a separate tool, enabled only for
models that want it. Selecting it by model belongs in 050's metadata.

## Diff output

Use `similar` (`TextDiff::from_lines`), unified format, 3 lines of context.
Also return `{ path, added, removed, hunks }` as structured data for the UI,
so the TUI does not have to parse diff text.

## Tests

- Unique match, multiple matches, no match, `replace_all`.
- CRLF file stays CRLF after edit.
- Whitespace fallback: tab-indented file, space-indented `old_string` →
  applied, re-indented with tabs, note in result.
- Edit without prior read → guard error. Read, external modify, edit → guard
  error.
- `multi_edit` where edit 2 of 3 fails → file unchanged.
- Preview diff equals the diff produced by the real edit.

## References

- `crush/internal/agent/tools/edit.go`, `multiedit.go`,
  `edit_whitespace.go`, and their `.md` tool descriptions (worth copying
  closely; the wording affects how well models use the tool).
- `opencode/packages/opencode/src/tool/edit.ts`: fuzzy replacer chain.
- `codex/codex-rs/apply-patch`.
