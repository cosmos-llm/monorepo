# 190 — Project instructions and environment context

**Status:** implemented — `PromptPlan` IR, `AGENTS.md`/`CLAUDE.md` loader (imports, caps, `*.local.md`, `load_dir_instructions`), environment probe/renderer and trongo goldens landed in `cosmos-llm-context`; deferred to `cosmos-llm-agent-core`: the read-tool hook that injects subdirectory instructions, TrongoScript prompt templates and `~/.config/cosmos/prompts/` overrides, `ir dump/check` CLI, `ModelInfo` (050) feeding model/cutoff, and swapping `PromptBlock`/`CacheHint` for the client's `ContentBlock`/`CacheHint` once 010/030 land.

**Crate:** `cosmos-llm-context` (loader, renderer) · **Depends on:** nothing · **Blocks:** first milestone

## Problem

The model needs to know the conventions of the repo it works in (build
commands, style rules, what not to touch) and basic facts about its
environment. Every reference agent loads an instructions file and adds
environment facts to the system prompt. `cosmos-llm-context` can compose
blocks but has no loader for either.

## Design

### Instruction files

Search order, each found file appended in this order (general → specific):

1. User-global: `~/.config/cosmos/AGENTS.md` (respect `XDG_CONFIG_HOME`).
2. Every directory from the git root (or filesystem root if not a repo) down
   to the cwd: `AGENTS.md`, then `CLAUDE.md` as a compatibility fallback.
   Configurable list, so `GEMINI.md` and `.cursorrules` can be added.
3. `AGENTS.local.md` / `CLAUDE.local.md` beside each, for untracked personal
   notes.

Rules:

- Size cap per file and in total (e.g. 32 KB). Over the cap: truncate with a
  note, and log it.
- `@path/to/file.md` imports (Claude Code supports them; opencode and crush
  read similar include syntax). Resolve relative to the including file,
  depth ≤ 5, no cycles, only inside the repo or the user config dir.
- Subdirectory files below the cwd: load lazily when the agent first reads a
  file in that directory, as Claude Code does. Otherwise a monorepo's 40
  `AGENTS.md` files all land in the prompt. Implement as a hook on the read
  tool that injects a system-reminder message. Second tier; the initial
  version can load only cwd and ancestors.

```rust
pub struct Instructions { pub files: Vec<LoadedFile>, pub truncated: bool }
pub fn load_instructions(cwd: &Path, opts: &InstructionOptions) -> io::Result<Instructions>;
```

### Environment block

Rendered once at thread start, refreshed when cwd changes:

- cwd, git root, whether it is a repo
- platform, OS version, shell
- today's date
- git: current branch, default branch, `git status --short` (capped), last 5
  commit subjects
- model name and knowledge cutoff, from 050

Keep it as a separate block from the instructions. It changes more often, and
placing it after the cached prefix (030) keeps the cache warm.

### System prompt assembly

The loader and the environment probe do not produce text. They produce a
`PromptPlan` IR (005), and rendering to text happens last:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PromptPlan { pub sections: Vec<PromptSection> }

pub struct PromptSection {
    pub kind: SectionKind,           // Base | ToolGuidance | Instructions | Environment | Reminder
    pub source: Option<PathBuf>,     // where it came from, for `/context` and debugging
    pub content: Vec<ContentBlock>,
    pub cache: Option<CacheHint>,    // 030
    pub stability: Stability,        // Static | Session | Turn — drives cache placement
}
```

Caching (030), token estimates (160) and compaction (170) all read the plan,
where the section boundaries are still there. The existing
`cosmos-llm-context` builder is the natural owner; `Block` already has a name
and metadata, and `PromptSection` can be its typed successor.

Default order:

```
[base agent prompt]        static, from the core crate
[tool usage guidance]      static per toolset
[instructions]             per project, stable within a session  ← cache breakpoint
[environment]              per session
```

The base prompt and tool guidance are TrongoScript templates (template mode,
005), with the plan's data (`model`, `tools`, `mode`, `os`) in scope. A model-
or mode-specific variant is then a conditional in the template rather than a
second copy of the prompt. Users can override a template from
`~/.config/cosmos/prompts/`.

The base prompt is product decisions (tone, when to ask, how to use the
tools). Start from pi-mono's minimal prompt rather than codex's long one, and
grow it from observed failures.

## Tests

- Fixture tree `/repo/AGENTS.md`, `/repo/pkg/CLAUDE.md`, cwd `/repo/pkg/sub`
  → both loaded, repo first.
- `@import` cycle → each file loaded once, no error.
- Over the size cap → truncated with note.
- Non-repo directory → walks to filesystem root and loads the global file
  only.

## References

- `codex/codex-rs/core/src/project_doc.rs` (AGENTS.md discovery).
- `crush/internal/config`: context paths list.
- `pi-mono/packages/coding-agent/src/core/`: system prompt and context files.
- `gemini-cli/packages/core/src/utils/memoryDiscovery.ts`: hierarchical
  discovery with imports.
