# 240 — Subagents

**Status:** implemented — `AgentSpec`/`AgentSet` (built-in `general` and `explore`; `.trongo` families and markdown), the `task` tool running a child `AgentLoop` through the parent's approver chain with an optional stricter `mode`, depth limit, derived cancel token, `Event::Subagent`, usage rolled into the parent's `RunFinished`, separate child session files linked from the result's `display`. Deferred: a `--max-cost` ceiling that spans children (usage is summed, not enforced), pre-approved grants per agent.

**Crate:** `cosmos-llm-agent-core` · **Depends on:** 060, 070, 150 · **Blocks:** nothing

## Problem

Broad searches ("find everywhere we parse config") fill the main context with
file dumps the main agent only needs the conclusion of. Running that work in a
child agent with its own context, and returning just its final answer, keeps
the parent's window for the actual task. Claude Code, opencode, crush and
codex all do this.

`AgentLoop` is already self-contained (registry, model, system prompt, caps),
so a child is cheap to construct. What is missing is the tool that starts
one, and the rules for what the child may do.

## Design

### `task` tool

| Param | Type | |
|---|---|---|
| `description` | string | 3–5 words, for the UI |
| `prompt` | string | full brief; the child sees nothing else |
| `agent` | string | which agent definition, default `general` |

The child runs `run_on` (070) with a fresh history and a derived
`CancellationToken` (interrupting the parent interrupts the child). Its final
text becomes the tool result, capped by 180.

### Agent definitions

Loaded like commands (230): `.trongo` preferred, markdown frontmatter for
compatibility, both lowering into an `AgentSpec` IR (005). In `.trongo`, one
file can define a family of agents:

```clojure
(defn reviewer [lang]
  {:name        (str "review-" lang)
   :description (str "Reviews " lang " changes")
   :tools       [:read :glob :grep]
   :model       "claude-haiku-5-5"
   :max-steps   30
   :prompt      (render "reviewer.erb" {:lang lang})})

{:agents (concat [{:name "explore"
                   :description "Read-only codebase search"
                   :tools [:read :glob :grep :list]
                   :model "claude-haiku-5-5"
                   :max-steps 30
                   :prompt (read-file "explore.md")}]
                 (map reviewer ["rust" "ruby" "typescript"]))}
```

`AgentSpec` holds names, not handles: tool names are resolved against the
parent's registry when the child starts.

Built-ins: `general` (parent's tools minus `task`) and `explore` (read-only).

### Rules

- **Tools:** a subset of the parent's registry. Never `task` itself, unless a
  depth limit (default 1) allows it.
- **Policy:** the child's tool calls go through the parent's approver (080)
  and policy (150). The user sees one approval stream, labelled with the
  child.
- **Budget:** usage rolls up into the parent's `TokenUsage` and `Session`
  budgets. `--max-cost` covers children.
- **Events:** child events are forwarded wrapped as `Event::Subagent {
  call_id, inner: Box<AgentEvent> }` so the UI can show nested progress or
  hide it.
- **Persistence:** the child's transcript is a separate session file (200)
  linked from the parent's tool call, not inlined.
- **Concurrency:** several `task` calls in one turn run in parallel, as other
  tools already do (`Executor::execute_all`). File edits from parallel
  children can conflict. The staleness guard (130) catches it, and the child
  just re-reads. Tracker state must be shared, not per child.

## Tests

- Parent calls `task`; mock child returns text; parent's tool result is that
  text and the parent history contains none of the child's messages.
- Child requesting a tool outside its subset gets "No such tool".
- Interrupt parent during child run → child's token fired, both histories
  valid.
- Child usage appears in parent totals.

## References

- `opencode/packages/opencode/src/tool/task.ts`, `agent/`.
- `crush/internal/agent` (agent tool).
- `codex/codex-rs/agent-roles`, `runtime-agent-instance`.
- `cosmos-llm-tool/src/session.rs`: budgets to share with children.
