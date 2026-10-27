//! Commands, hooks and subagents wired into the core (230, 240).
//!
//! A child module of [`core`](super) so it can reach the shared state without
//! widening what the rest of the crate sees. The pieces themselves live in
//! [`command`](crate::command), [`hooks`](crate::hooks) and
//! [`subagent`](crate::subagent).

use super::*;

use crate::command::{CommandSet, CommandSpec, Expansion};
use crate::hooks::{HookEvent, HookRunner, HookSpec};
use crate::subagent::{AgentSet, SubagentHost, SubagentOptions};

use cosmos_llm::ContentBlock;
use cosmos_llm_tool::StopReason;

/// How many times a `stop` hook may ask the agent to keep going per input.
pub(super) const MAX_STOP_CONTINUATIONS: usize = 3;

/// What the builder collects before `start`.
#[derive(Default)]
pub(super) struct BuilderExt {
    pub hooks: Vec<HookSpec>,
    pub commands: CommandSet,
    pub agents: Option<AgentSet>,
    pub subagents: Option<SubagentOptions>,
}

/// The core's extension state, held in `Shared`.
pub(super) struct Ext {
    pub hooks: Option<Arc<HookRunner>>,
    pub commands: Arc<CommandSet>,
    pub subagents: Option<Arc<SubagentHost>>,
    pub cwd: PathBuf,
    /// While set, `RunFinished` is held back so a `stop` hook can extend the run.
    suppress_finish: AtomicBool,
    held_finish: Mutex<Option<Event>>,
    /// Context from `session_start` hooks, delivered with the first input.
    session_context: Mutex<Option<String>>,
}

impl Ext {
    pub fn new(
        hooks: Option<Arc<HookRunner>>,
        commands: Arc<CommandSet>,
        subagents: Option<Arc<SubagentHost>>,
        cwd: PathBuf,
    ) -> Self {
        Self {
            hooks,
            commands,
            subagents,
            cwd,
            suppress_finish: AtomicBool::new(false),
            held_finish: Mutex::new(None),
            session_context: Mutex::new(None),
        }
    }

    /// Called by `Shared::emit`: keeps a `RunFinished` back when asked to.
    pub fn hold(&self, event: &Event) -> bool {
        if matches!(event, Event::RunFinished { .. }) && self.suppress_finish.load(Ordering::SeqCst)
        {
            *lock(&self.held_finish) = Some(event.clone());
            return true;
        }
        false
    }

    /// Adds what subagents used during the run to the run's own usage.
    pub fn rollup(&self, mut usage: TokenUsage) -> TokenUsage {
        if let Some(h) = &self.subagents {
            usage.add(&h.take_usage());
        }
        usage
    }
}

fn text_of(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Fires the `session_start` hooks; their context joins the first input.
pub(super) async fn session_start(shared: &Shared) {
    let Some(h) = shared
        .ext
        .hooks
        .as_ref()
        .filter(|h| h.has(HookEvent::SessionStart))
    else {
        return;
    };
    let out = h
        .fire(
            &h.payload(HookEvent::SessionStart),
            &CancellationToken::new(),
        )
        .await;
    if let Some(reason) = out.block {
        shared
            .warn(format!("a SessionStart hook objected: {reason}"))
            .await;
    }
    if !out.context.is_empty() {
        *lock(&shared.ext.session_context) = Some(out.context.join("\n\n"));
    }
}

/// One input, with the `user_prompt_submit` and `stop` hooks around it.
pub(super) async fn hooked_turn(
    shared: Arc<Shared>,
    agent: Arc<AgentLoop>,
    mut o: Owned,
    mut blocks: Vec<ContentBlock>,
    cancel: CancellationToken,
) -> Owned {
    let hooks = shared.ext.hooks.clone();
    if let Some(ctx) = lock(&shared.ext.session_context).take() {
        blocks.insert(
            0,
            ContentBlock::text(format!(
                "<session-start-hook>\n{ctx}\n</session-start-hook>"
            )),
        );
    }
    if let Some(h) = hooks
        .as_ref()
        .filter(|h| h.has(HookEvent::UserPromptSubmit))
    {
        let mut p = h.payload(HookEvent::UserPromptSubmit);
        p.prompt = Some(text_of(&blocks));
        let out = h.fire(&p, &cancel).await;
        if let Some(reason) = out.block {
            shared
                .warn(format!(
                    "input blocked by a UserPromptSubmit hook: {reason}"
                ))
                .await;
            shared
                .emit(Event::RunFinished {
                    reason: None,
                    steps: 0,
                    usage: TokenUsage::default(),
                })
                .await;
            return o;
        }
        for c in out.context {
            blocks.push(ContentBlock::text(format!(
                "<user-prompt-submit-hook>\n{c}\n</user-prompt-submit-hook>"
            )));
        }
    }

    let Some(stop) = hooks.filter(|h| h.has(HookEvent::Stop)) else {
        return run_turn(shared, agent, o, blocks, cancel).await;
    };

    // With a stop hook, hold each run's `RunFinished` until the hook has had
    // its say, and report one for the whole input.
    let mut total = TokenUsage::default();
    let mut steps = 0u32;
    let mut next = blocks;
    let mut round = 0;
    loop {
        shared.ext.suppress_finish.store(true, Ordering::SeqCst);
        o = run_turn(
            Arc::clone(&shared),
            Arc::clone(&agent),
            o,
            next,
            cancel.clone(),
        )
        .await;
        shared.ext.suppress_finish.store(false, Ordering::SeqCst);
        let held = lock(&shared.ext.held_finish).take();
        let (mut reason, mut usage) = (None, TokenUsage::default());
        if let Some(Event::RunFinished {
            reason: r,
            steps: s,
            usage: u,
        }) = held
        {
            reason = r;
            usage = u;
            steps += s;
        }
        total.add(&usage);

        if reason == Some(StopReason::Finished)
            && !cancel.is_cancelled()
            && round < MAX_STOP_CONTINUATIONS
        {
            let mut p = stop.payload(HookEvent::Stop);
            p.stop_hook_active = round > 0;
            p.assistant_message = o
                .thread
                .history()
                .iter()
                .rev()
                .find(|m| m.role == Role::Assistant)
                .map(|m| m.text());
            let out = stop.fire(&p, &cancel).await;
            if let Some(why) = out.block {
                round += 1;
                shared
                    .warn(format!("a Stop hook asked the agent to continue: {why}"))
                    .await;
                next = vec![ContentBlock::text(format!(
                    "A Stop hook asked you to continue: {why}"
                ))];
                continue;
            }
        }
        shared
            .emit(Event::RunFinished {
                reason,
                steps,
                usage: total,
            })
            .await;
        return o;
    }
}

fn expansion(shared: &Shared) -> Expansion {
    Expansion::new(shared.ext.cwd.clone()).engine(Arc::clone(&shared.engine))
}

/// `Op::RunCommand` with no run in progress: expand, then run as an input.
pub(super) async fn command_turn(
    shared: Arc<Shared>,
    agent: Arc<AgentLoop>,
    o: Owned,
    spec: CommandSpec,
    args: String,
    cancel: CancellationToken,
) -> Owned {
    let expanded = spec.expand(&args, &expansion(&shared)).await;
    for w in expanded.warnings {
        shared.warn(w).await;
    }
    let previous = spec
        .model
        .as_ref()
        .map(|m| std::mem::replace(&mut *lock(&shared.completer.model), m.clone()));
    let o = hooked_turn(
        Arc::clone(&shared),
        agent,
        o,
        vec![ContentBlock::text(expanded.text)],
        cancel,
    )
    .await;
    if let Some(p) = previous {
        *lock(&shared.completer.model) = p;
    }
    o
}

/// `Op::RunCommand` while a run is in progress: expand off the op loop, then
/// steer the run with the result.
pub(super) fn queue_command(
    shared: Arc<Shared>,
    queue: InputQueue,
    spec: CommandSpec,
    args: String,
) {
    tokio::spawn(async move {
        let expanded = spec.expand(&args, &expansion(&shared)).await;
        for w in expanded.warnings {
            shared.warn(w).await;
        }
        queue.push(UserInput::from_blocks(vec![ContentBlock::text(
            expanded.text,
        )]));
    });
}

/// Looks `name` up, or reports that it is unknown.
pub(super) async fn resolve_command(shared: &Shared, name: &str) -> Option<CommandSpec> {
    match shared.ext.commands.get(name) {
        Some(spec) => Some(spec.clone()),
        None => {
            shared
                .error(
                    format!("unknown command: {}", name.trim_start_matches('/')),
                    false,
                )
                .await;
            None
        }
    }
}
