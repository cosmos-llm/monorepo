//! The core (210): one task owns the thread, turns run as child tasks.
//!
//! ```text
//!  frontend ──Op──▶ CoreHandle ──▶ op loop ──spawn──▶ turn task
//!     ▲                              │  owns Thread        │ AgentLoop::run_queued
//!     └──────────Event◀── bridge ◀───┴── AgentEvent ◀──────┘
//!                           └─▶ session log (200), ContextUsage (160)
//! ```
//!
//! The op loop never blocks on a turn, so `Interrupt` and `Approve` stay
//! responsive. Approvals flow through a chain: [`PolicyApprover`] (150) first,
//! then the [`InteractiveApprover`], which parks the loop on a oneshot until
//! `Op::Approve` arrives.
//!
//! Compaction (170) is callable through [`Compactor`]; headless mode (220) is a
//! frontend of this protocol. Commands, hooks (230) and subagents (240) are
//! wired in by the private `ext` module: `Op::RunCommand` expands a
//! [`CommandSpec`]; tool hooks wrap the tool handlers; the `task` tool starts a
//! child loop behind the same approver chain.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cosmos_llm::{
    CancellationToken, CompletionRequest, CompletionResponse, CosmosError, Message, ModelInfo,
    Reasoning, Role, TokenEstimator,
};
use cosmos_llm_coding_tools::{CodingTools, SharedTracker};
use cosmos_llm_context::{
    load_instructions, probe_environment, InstructionOptions, Instructions, ProbeOptions,
    PromptPlan,
};
use cosmos_llm_tool::fs::ToolFs;
use cosmos_llm_tool::{
    event_channel, AgentEvent, AgentLoop, ApprovalRequest, Approver, Completer, Decision,
    InputQueue, Registry, RunHandles, SpillStore, Thread, TokenUsage, UserInput,
};
use futures_util::future::BoxFuture;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::approval::InteractiveApprover;
use crate::budget::{context_tokens, ContextBudget, ContextTracker, Pressure};
use crate::compaction::{CompactionPlan, CompactionReason, Compactor};
use crate::command::{CommandSet, CommandSpec};
use crate::config::CoreConfig;
use crate::hooks::HookRunner;
use crate::subagent::SubagentHost;
use crate::error::{CoreError, Result};
use crate::policy::{PolicyApprover, PolicyEngine, ShellParser};
use crate::protocol::{Event, Op, PROTOCOL_VERSION};
use crate::record::{FileChange, MsgId, Record};
use crate::session::{
    tool_output_dir, write_blob, LoadedSession, SessionLog, SessionStore, SessionWriter,
};

const DEFAULT_BASE_PROMPT: &str = "You are a coding agent working in the user's repository. \
Use the available tools to read, search, edit and run code. Prefer small, verifiable changes, \
and say plainly when you are unsure.";

mod ext;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Which saved session to pick up.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ResumeFrom {
    /// The newest session for the workspace (`--continue`).
    Latest,
    /// A session id or unique prefix (`--resume`).
    Id(String),
    /// A log file.
    Path(PathBuf),
}

/// Applies the live model and reasoning settings to every request, so
/// `SetModel` and `SetReasoning` need no rebuild of the loop (and so the
/// loop's own state, such as approval grants, survives them).
struct SettingsCompleter {
    inner: Arc<dyn Completer>,
    model: Mutex<String>,
    reasoning: Mutex<Option<Reasoning>>,
}

impl SettingsCompleter {
    fn apply(&self, req: &CompletionRequest) -> CompletionRequest {
        let mut r = req.clone();
        let model = lock(&self.model).clone();
        if !model.is_empty() {
            r.model = model;
        }
        if let Some(reasoning) = *lock(&self.reasoning) {
            r = r.with_reasoning(reasoning);
        }
        r
    }
}

impl Completer for SettingsCompleter {
    fn completion<'a>(
        &'a self,
        req: &'a CompletionRequest,
    ) -> BoxFuture<'a, std::result::Result<CompletionResponse, CosmosError>> {
        let r = self.apply(req);
        Box::pin(async move { self.inner.completion(&r).await })
    }

    fn stream<'a>(
        &'a self,
        req: &'a CompletionRequest,
    ) -> BoxFuture<'a, std::result::Result<cosmos_llm::CompletionStream, CosmosError>> {
        let r = self.apply(req);
        Box::pin(async move { self.inner.stream(&r).await })
    }
}

/// Builds a core from config plus the live pieces config cannot hold.
pub struct CoreBuilder {
    config: CoreConfig,
    completer: Option<Arc<dyn Completer>>,
    registry: Registry,
    tracking: Option<(SharedTracker, Arc<dyn ToolFs>)>,
    compactor: Option<Arc<dyn Compactor>>,
    parser: Option<Arc<dyn ShellParser>>,
    store: Option<SessionStore>,
    resume: Option<ResumeFrom>,
    estimator: Option<Arc<dyn TokenEstimator>>,
    warnings: Vec<String>,
    ext: ext::BuilderExt,
}

/// Entry point: [`Core::builder`] or [`Core::start`].
pub struct Core;

impl Core {
    /// Starts a builder.
    pub fn builder(config: CoreConfig) -> CoreBuilder {
        CoreBuilder {
            config,
            completer: None,
            registry: Registry::new(),
            tracking: None,
            compactor: None,
            parser: None,
            store: None,
            resume: None,
            estimator: None,
            warnings: Vec::new(),
            ext: ext::BuilderExt::default(),
        }
    }

    /// Starts a core with `completer` and `registry` and everything else
    /// default. Must be called inside a tokio runtime.
    pub fn start(
        config: CoreConfig,
        completer: Arc<dyn Completer>,
        registry: Registry,
    ) -> Result<(CoreHandle, mpsc::Receiver<Event>)> {
        Self::builder(config)
            .completer(completer)
            .tools(registry)
            .start()
    }
}

impl CoreBuilder {
    /// The client (or any [`Completer`]) the loop talks to.
    #[must_use]
    pub fn completer(mut self, completer: Arc<dyn Completer>) -> Self {
        self.completer = Some(completer);
        self
    }

    /// The tools the model may call. Keep no other clone of tools that own
    /// background jobs: they are killed when the core drops its registry.
    #[must_use]
    pub fn tools(mut self, registry: Registry) -> Self {
        self.registry = registry;
        self
    }

    /// Registers all of `tools` and turns on file tracking, turn diffs and
    /// undo with its tracker.
    #[must_use]
    pub fn coding_tools(mut self, tools: &CodingTools) -> Self {
        tools.register_all(&mut self.registry);
        let fs: Arc<dyn ToolFs> = tools.fs();
        self.tracking = Some((tools.tracker(), fs));
        self
    }

    /// Turns on turn diffs and undo for a tracker the host manages.
    #[must_use]
    pub fn file_tracking(mut self, tracker: SharedTracker, fs: Arc<dyn ToolFs>) -> Self {
        self.tracking = Some((tracker, fs));
        self
    }

    /// Installs the compactor (170).
    #[must_use]
    pub fn compactor(mut self, compactor: Arc<dyn Compactor>) -> Self {
        self.compactor = Some(compactor);
        self
    }

    /// Replaces the shell parser the policy classifies with.
    #[must_use]
    pub fn shell_parser(mut self, parser: Arc<dyn ShellParser>) -> Self {
        self.parser = Some(parser);
        self
    }

    /// Uses this store instead of the one config names.
    #[must_use]
    pub fn session_store(mut self, store: SessionStore) -> Self {
        self.store = Some(store);
        self
    }

    /// Picks up a saved session instead of starting a new one.
    #[must_use]
    pub fn resume(mut self, from: ResumeFrom) -> Self {
        self.resume = Some(from);
        self
    }

    /// Replaces the token estimator used for the unanchored tail.
    #[must_use]
    pub fn estimator(mut self, estimator: Arc<dyn TokenEstimator>) -> Self {
        self.estimator = Some(estimator);
        self
    }

    /// Registers hooks (230). Project hooks need trust first; see
    /// [`LoadedHooks`](crate::LoadedHooks).
    #[must_use]
    pub fn hooks(mut self, hooks: Vec<crate::hooks::HookSpec>) -> Self {
        self.ext.hooks = hooks;
        self
    }

    /// Sets the custom commands `Op::RunCommand` can run (230).
    #[must_use]
    pub fn commands(mut self, commands: CommandSet) -> Self {
        self.ext.commands = commands;
        self
    }

    /// Sets the agents the `task` tool can start (240). The default is the
    /// built-in `general` and `explore`.
    #[must_use]
    pub fn agents(mut self, agents: crate::subagent::AgentSet) -> Self {
        self.ext.agents = Some(agents);
        self
    }

    /// Sets subagent limits (240). `SubagentOptions::depth(0)` leaves out the
    /// `task` tool.
    #[must_use]
    pub fn subagents(mut self, opts: crate::subagent::SubagentOptions) -> Self {
        self.ext.subagents = Some(opts);
        self
    }

    /// Loads commands, agents and hooks from the user and project config
    /// directories `opts` names, and queues what went wrong as warnings.
    /// Untrusted project hooks are left out (see [`LoadedHooks`](crate::LoadedHooks));
    /// pass a [`HookTrust`](crate::HookTrust) to honour earlier approvals.
    #[must_use]
    pub fn extensions(
        mut self,
        opts: &crate::config::LoadOptions,
        trust: Option<&crate::hooks::HookTrust>,
    ) -> Self {
        let (commands, w) = CommandSet::load(opts);
        self.warnings.extend(w);
        let (agents, w) = crate::subagent::AgentSet::load(opts);
        self.warnings.extend(w);
        let hooks = crate::hooks::LoadedHooks::load(opts, trust);
        self.warnings.extend(hooks.warnings);
        self.ext.commands = commands;
        self.ext.agents = Some(agents);
        self.ext.hooks = hooks.active;
        self
    }

    /// Adds a warning to show as the first event (for example, the warnings
    /// from [`CoreConfig::load`]).
    #[must_use]
    pub fn warnings<I: IntoIterator<Item = String>>(mut self, w: I) -> Self {
        self.warnings.extend(w);
        self
    }

    /// Spawns the core. Call inside a tokio runtime.
    ///
    /// # Errors
    ///
    /// Fails when no completer or model is set, or a session cannot be
    /// opened or resumed.
    pub fn start(self) -> Result<(CoreHandle, mpsc::Receiver<Event>)> {
        let CoreBuilder {
            config,
            completer,
            registry,
            tracking,
            compactor,
            parser,
            store,
            resume,
            estimator,
            mut warnings,
            ext: builder_ext,
        } = self;
        let completer = completer.ok_or_else(|| CoreError::Setup("no completer".into()))?;
        let cwd = match &config.cwd {
            Some(c) => c.clone(),
            None => std::env::current_dir().map_err(|e| CoreError::io(".", e))?,
        };
        let cwd = cwd.canonicalize().unwrap_or(cwd);

        // Session: resume or create.
        let store = if config.sessions.enabled {
            store.or_else(|| {
                config
                    .sessions
                    .dir
                    .clone()
                    .or_else(SessionStore::default_root)
                    .map(SessionStore::new)
            })
        } else {
            None
        };
        if let (Some(store), Some(days)) = (&store, config.sessions.retention_days) {
            if let Err(e) = store.apply_retention(days) {
                warnings.push(format!("session retention failed: {e}"));
            }
        }
        let mut loaded: Option<LoadedSession> = None;
        if let Some(from) = &resume {
            let store = store.as_ref().ok_or_else(|| {
                CoreError::Setup("sessions are disabled; nothing to resume".into())
            })?;
            let path = match from {
                ResumeFrom::Latest => {
                    store
                        .latest(&cwd)?
                        .ok_or_else(|| CoreError::NoSuchSession("(latest)".into()))?
                        .path
                }
                ResumeFrom::Id(id) => store.find(&cwd, id)?.path,
                ResumeFrom::Path(p) => p.clone(),
            };
            let s = SessionLog::load(&path)?;
            warnings.extend(s.warnings.iter().cloned());
            loaded = Some(s);
        }

        let model = config
            .model
            .clone()
            .or_else(|| loaded.as_ref().map(|l| l.model.clone()))
            .ok_or_else(|| CoreError::Setup("no model configured".into()))?;
        let compactor = compactor.or_else(|| {
            crate::compaction::default_compactor(
                &config.compaction,
                &completer,
                &model,
                &mut warnings,
            )
        });

        let writer = match (&store, &loaded) {
            (Some(store), Some(l)) => Some(Arc::new(store.reopen(&l.path)?)),
            (Some(store), None) => Some(Arc::new(store.create(&cwd, &model)?)),
            _ => None,
        };

        // Policy and the approver chain.
        let mut engine = PolicyEngine::new(config.permissions.clone(), &cwd)
            .with_remember_dir(cwd.join(".cosmos"));
        if let Some(p) = parser {
            engine = engine.with_parser(p);
        }
        let engine = Arc::new(engine);
        let (interactive, asked_rx) = InteractiveApprover::new();
        let (note_tx, note_rx) = mpsc::unbounded_channel();
        let chain: Arc<dyn Approver> = Arc::new(
            PolicyApprover::new(Arc::clone(&engine), interactive.clone()).with_notes(note_tx.clone()),
        );

        // Hooks wrap the tools; the `task` tool (240) is added on top, so
        // children get the wrapped tools without it.
        let (ev_tx, ev_rx) = mpsc::channel(config.event_buffer.max(16));
        let raw_completer = Arc::clone(&completer);
        let settings = Arc::new(SettingsCompleter {
            inner: completer,
            model: Mutex::new(model.clone()),
            reasoning: Mutex::new(config.reasoning),
        });
        let hook_runner = (!builder_ext.hooks.is_empty()).then(|| {
            Arc::new(HookRunner::new(builder_ext.hooks.clone(), &cwd).with_warnings(note_tx))
        });
        let registry = match &hook_runner {
            Some(h) => h.wrap_registry(registry),
            None => registry,
        };
        let sub_opts = builder_ext.subagents.unwrap_or_default();
        let subagent_host = (sub_opts.max_depth > 0).then(|| {
            let mut base = Registry::new();
            for t in registry.all().into_iter().filter(|t| t.name != "task") {
                base.register(t.clone());
            }
            Arc::new(SubagentHost {
                agents: Arc::new(builder_ext.agents.clone().unwrap_or_default()),
                base: Arc::new(base),
                parent_completer: settings.clone(),
                raw_completer,
                events: ev_tx.clone(),
                chain: Arc::clone(&chain),
                interactive: interactive.clone(),
                engine: Arc::clone(&engine),
                depth: 0,
                max_depth: sub_opts.max_depth,
                prefix: String::new(),
                usage: Arc::new(Mutex::new(TokenUsage::default())),
                log_root: writer
                    .as_ref()
                    .map(|w| crate::session::aux_dir(w.path()).join("subagents")),
                cwd: cwd.clone(),
                default_max_steps: config.max_steps,
            })
        });
        let mut registry = registry;
        if let Some(h) = &subagent_host {
            registry.register(h.task_tool());
        }

        // Prompt, loop.
        let system = build_system_prompt(&config, &cwd, &model);
        let mut agent = AgentLoop::new(registry)
            .with_system(system.clone())
            .with_max_steps(config.max_steps)
            .with_approver(chain);
        if let Some(w) = &writer {
            agent = agent.with_spill(SpillStore::new(tool_output_dir(w.path())));
        }
        let system_tokens = cosmos_llm::tokens::estimate_text_tokens(&system).saturating_add(
            agent
                .tool_specs()
                .iter()
                .map(|s| cosmos_llm::tokens::estimate_text_tokens(&s.to_json_schema().to_string()))
                .sum::<u32>(),
        );
        let agent = Arc::new(agent);

        let info = config.model_info(&model);
        let budget = ContextBudget::for_model(info.as_ref(), &config.context);
        let ctx = match estimator {
            Some(e) => ContextTracker::new(e, system_tokens),
            None => ContextTracker::new(Arc::new(cosmos_llm::CharsEstimator), system_tokens),
        };

        let (op_tx, op_rx) = mpsc::channel(256);

        let (thread, msg_ids, next_id) = match &mut loaded {
            Some(l) => {
                let t = std::mem::replace(&mut l.thread, Thread::new(l.id.to_string()));
                (t, l.msg_ids.clone(), l.next_id)
            }
            None => {
                let id = writer
                    .as_ref()
                    .map_or_else(uuid::Uuid::new_v4, |w| w.id())
                    .to_string();
                (Thread::new(id), Vec::new(), 1)
            }
        };
        let session_id = thread.id().0.clone();
        let commands = Arc::new(builder_ext.commands);
        if let Some(h) = &hook_runner {
            h.set_session_id(&session_id);
        }
        let history_items = loaded.as_ref().map(|l| l.display.clone());
        let log_path = writer.as_ref().map(|w| w.path().to_owned());

        let shared = Arc::new(Shared {
            events: ev_tx,
            completer: settings,
            ext: ext::Ext::new(
                hook_runner,
                Arc::clone(&commands),
                subagent_host,
                cwd.clone(),
            ),
            engine,
            interactive,
            asked_rx: tokio::sync::Mutex::new(asked_rx),
            writer,
            tracking,
            budget: Mutex::new(budget),
            ctx: Mutex::new(ctx),
            compactor,
            warned_no_compactor: AtomicBool::new(false),
            config,
        });

        // Notes from the policy chain ("remembered in ...") become warnings.
        // (It holds the event sender, not `Shared`: the hook runner in `Shared`
        // owns a sender of these notes, and a task holding `Shared` would keep
        // the event channel open after shutdown.)
        {
            let events = shared.events.clone();
            let mut note_rx = note_rx;
            tokio::spawn(async move {
                while let Some(message) = note_rx.recv().await {
                    let _ = events.send(Event::Warning { message }).await;
                }
            });
        }

        let owned = Owned {
            thread,
            sess: SessionState { msg_ids, next_id },
        };
        let first = Event::SessionConfigured {
            protocol: PROTOCOL_VERSION,
            session_id,
            model,
            cwd,
            mode: shared.engine.mode(),
            context_window: lock(&shared.budget).map(|b| b.window),
            log_path,
        };
        tokio::spawn(run_core(
            shared,
            agent,
            op_rx,
            owned,
            first,
            history_items,
            warnings,
        ));
        Ok((CoreHandle { tx: op_tx, commands }, ev_rx))
    }
}

fn build_system_prompt(cfg: &CoreConfig, cwd: &Path, model: &str) -> String {
    let base = cfg
        .system_prompt
        .clone()
        .unwrap_or_else(|| DEFAULT_BASE_PROMPT.to_owned());
    if !cfg.instructions {
        return base;
    }
    let instructions: Instructions =
        load_instructions(cwd, &InstructionOptions::default()).unwrap_or_default();
    let mut probe = ProbeOptions::default();
    probe.model = Some(model.to_owned());
    let env = probe_environment(cwd, &probe);
    PromptPlan::assemble(&base, "", &instructions, Some(&env)).render()
}

/// A clonable sender of [`Op`]s.
#[derive(Debug, Clone)]
pub struct CoreHandle {
    tx: mpsc::Sender<Op>,
    commands: Arc<CommandSet>,
}

impl CoreHandle {
    /// Sends an op. Fails when the core has shut down.
    pub async fn send(&self, op: Op) -> Result<()> {
        self.tx
            .send(op)
            .await
            .map_err(|_| CoreError::Setup("the core has shut down".into()))
    }

    /// Sends a text-only `UserInput`.
    pub async fn user_text(&self, text: impl Into<String>) -> Result<()> {
        self.send(Op::UserInput {
            blocks: vec![cosmos_llm::ContentBlock::text(text)],
        })
        .await
    }

    /// The custom commands (230) for a completion popup, sorted by name.
    pub fn list_commands(&self) -> Vec<CommandSpec> {
        self.commands.list()
    }

    /// Sends `Interrupt`.
    pub async fn interrupt(&self) -> Result<()> {
        self.send(Op::Interrupt).await
    }

    /// Answers an approval.
    pub async fn approve(&self, call_id: impl Into<String>, decision: Decision) -> Result<()> {
        self.send(Op::Approve {
            call_id: call_id.into(),
            decision,
            remember: false,
        })
        .await
    }

    /// Sends `Shutdown`.
    pub async fn shutdown(&self) -> Result<()> {
        self.send(Op::Shutdown).await
    }
}

struct Shared {
    events: mpsc::Sender<Event>,
    completer: Arc<SettingsCompleter>,
    engine: Arc<PolicyEngine>,
    interactive: Arc<InteractiveApprover>,
    asked_rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<ApprovalRequest>>,
    writer: Option<Arc<SessionWriter>>,
    tracking: Option<(SharedTracker, Arc<dyn ToolFs>)>,
    budget: Mutex<Option<ContextBudget>>,
    ctx: Mutex<ContextTracker>,
    compactor: Option<Arc<dyn Compactor>>,
    warned_no_compactor: AtomicBool,
    config: CoreConfig,
    ext: ext::Ext,
}

impl Shared {
    async fn emit(&self, event: Event) {
        if self.ext.hold(&event) {
            return;
        }
        let _ = self.events.send(event).await;
    }

    async fn warn(&self, message: impl Into<String>) {
        self.emit(Event::Warning {
            message: message.into(),
        })
        .await;
    }

    async fn error(&self, message: impl Into<String>, fatal: bool) {
        self.emit(Event::Error {
            message: message.into(),
            fatal,
        })
        .await;
    }

    fn budget(&self) -> Option<ContextBudget> {
        *lock(&self.budget)
    }

    fn log(&self, record: &Record) -> Option<String> {
        self.writer
            .as_ref()
            .and_then(|w| w.append(record).err())
            .map(|e| format!("session log: {e}"))
    }

    async fn report_context(&self, used: u32) -> Pressure {
        match self.budget() {
            Some(b) => {
                let pressure = b.pressure(used);
                self.emit(Event::ContextUsage {
                    used,
                    window: b.window,
                    pressure,
                })
                .await;
                pressure
            }
            None => Pressure::Ok,
        }
    }
}

/// Bookkeeping that follows the thread: the log id of each history message.
struct SessionState {
    msg_ids: Vec<MsgId>,
    next_id: MsgId,
}

struct Owned {
    thread: Thread,
    sess: SessionState,
}

/// Appends history messages the log has not seen. Usages are matched to the
/// new assistant messages in order. Returns the first logging error.
fn persist(
    shared: &Shared,
    sess: &mut SessionState,
    history: &[Message],
    usages: &mut VecDeque<TokenUsage>,
) -> Option<String> {
    if history.len() < sess.msg_ids.len() {
        sess.msg_ids.truncate(history.len());
    }
    let mut err = None;
    for m in &history[sess.msg_ids.len()..] {
        let id = sess.next_id;
        sess.next_id += 1;
        sess.msg_ids.push(id);
        let usage = if m.role == Role::Assistant {
            usages.pop_front()
        } else {
            None
        };
        if let Some(w) = &shared.writer {
            if let Err(e) = w.append_message(id, m, usage) {
                err.get_or_insert(format!("session log: {e}"));
            }
        }
    }
    err
}

async fn run_core(
    shared: Arc<Shared>,
    agent: Arc<AgentLoop>,
    mut ops: mpsc::Receiver<Op>,
    owned: Owned,
    first: Event,
    history: Option<Vec<crate::record::DisplayItem>>,
    warnings: Vec<String>,
) {
    shared.emit(first).await;
    if let Some(items) = history {
        shared.emit(Event::HistoryLoaded { items }).await;
    }
    for w in warnings {
        shared.warn(w).await;
    }
    ext::session_start(&shared).await;

    let mut owned = Some(owned);
    struct Running {
        handle: JoinHandle<Owned>,
        cancel: CancellationToken,
        queue: InputQueue,
    }
    let mut running: Option<Running> = None;

    loop {
        let finished = async {
            match running.as_mut() {
                Some(r) => (&mut r.handle).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            done = finished => {
                running = None;
                match done {
                    Ok(o) => owned = Some(o),
                    Err(e) => {
                        shared.error(format!("the turn task failed: {e}"), true).await;
                        break;
                    }
                }
            }
            op = ops.recv() => {
                let Some(op) = op else { break };
                match op {
                    Op::UserInput { blocks } => {
                        if let Some(r) = &running {
                            r.queue.push(UserInput::from_blocks(blocks));
                        } else if let Some(o) = owned.take() {
                            let cancel = CancellationToken::new();
                            let queue = o.thread.queue();
                            let handle = tokio::spawn(ext::hooked_turn(
                                Arc::clone(&shared),
                                Arc::clone(&agent),
                                o,
                                blocks,
                                cancel.clone(),
                            ));
                            running = Some(Running { handle, cancel, queue });
                        }
                    }
                    Op::Interrupt => {
                        if let Some(r) = &running {
                            r.cancel.cancel();
                        }
                    }
                    Op::Approve { call_id, decision, remember } => {
                        if remember {
                            shared.engine.flag_remember(&call_id);
                        }
                        shared.interactive.resolve(&call_id, decision);
                    }
                    Op::SetModel { model } => {
                        *lock(&shared.completer.model) = model.clone();
                        let info = shared.config.model_info(&model);
                        let budget = ContextBudget::for_model(info.as_ref(), &shared.config.context);
                        *lock(&shared.budget) = budget;
                        if let Some(e) = shared.log(&Record::ModelChanged { model: model.clone() }) {
                            shared.warn(e).await;
                        }
                        shared.emit(Event::ModelChanged {
                            model,
                            context_window: budget.map(|b| b.window),
                        }).await;
                    }
                    Op::SetMode { mode } => {
                        shared.engine.set_mode(mode);
                        shared.emit(Event::ModeChanged { mode }).await;
                    }
                    Op::SetReasoning { reasoning } => {
                        *lock(&shared.completer.reasoning) = Some(reasoning);
                    }
                    Op::Compact { instructions } => match owned.as_mut() {
                        Some(o) => {
                            if let Err(e) = compact(&shared, o, CompactionReason::Manual, instructions).await {
                                shared.error(e, false).await;
                            }
                        }
                        None => shared.warn("busy: compact after the turn ends").await,
                    },
                    Op::Undo { turn } => {
                        if owned.is_some() {
                            undo(&shared, turn).await;
                        } else {
                            shared.warn("busy: undo after the turn ends").await;
                        }
                    }
                    Op::TruncateTo { message } => match owned.as_mut() {
                        Some(o) => truncate_to(&shared, o, message).await,
                        None => shared.warn("busy: edit after the turn ends").await,
                    },
                    Op::RunCommand { name, args } => {
                        if let Some(spec) = ext::resolve_command(&shared, &name).await {
                            if let Some(r) = &running {
                                ext::queue_command(Arc::clone(&shared), r.queue.clone(), spec, args);
                            } else if let Some(o) = owned.take() {
                                let cancel = CancellationToken::new();
                                let queue = o.thread.queue();
                                let handle = tokio::spawn(ext::command_turn(
                                    Arc::clone(&shared),
                                    Arc::clone(&agent),
                                    o,
                                    spec,
                                    args,
                                    cancel.clone(),
                                ));
                                running = Some(Running { handle, cancel, queue });
                            }
                        }
                    }
                    Op::Shutdown => break,
                }
            }
        }
    }

    // Shutdown: stop the turn, let it settle, flush the log, then drop the
    // tools (which kills their background jobs) before saying goodbye.
    if let Some(r) = running.take() {
        r.cancel.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(5), r.handle).await;
    }
    if let Some(w) = &shared.writer {
        if let Err(e) = w.sync().await {
            shared.warn(format!("session log: {e}")).await;
        }
    }
    drop(agent);
    shared.emit(Event::ShutdownComplete).await;
}

async fn truncate_to(shared: &Shared, o: &mut Owned, message: MsgId) {
    let Some(pos) = o.sess.msg_ids.iter().position(|id| *id == message) else {
        shared
            .error(format!("no message with id {message}"), false)
            .await;
        return;
    };
    if let Err(e) = o.thread.truncate_to(pos) {
        shared.error(e.to_string(), false).await;
        return;
    }
    o.sess.msg_ids.truncate(pos);
    let after = pos.checked_sub(1).map_or(0, |p| o.sess.msg_ids[p]);
    if let Some(e) = shared.log(&Record::Truncated { after }) {
        shared.warn(e).await;
    }
    lock(&shared.ctx).invalidate();
    let used = lock(&shared.ctx).estimate(o.thread.history());
    shared.report_context(used).await;
}

async fn undo(shared: &Shared, turn: Option<u32>) {
    let Some((tracker, fs)) = &shared.tracking else {
        shared
            .warn("file tracking is not enabled; nothing to undo")
            .await;
        return;
    };
    let outcome = {
        let mut t = lock_tracker(tracker);
        let turn = turn.or_else(|| {
            t.all_changes()
                .iter()
                .rev()
                .find(|c| !c.files.is_empty())
                .map(|c| c.turn)
        });
        match turn {
            None => Err("no recorded file changes to undo".to_owned()),
            Some(turn) => t
                .revert_turn(fs.as_ref(), turn, false)
                .map(|r| (turn, r))
                .map_err(|e| e.to_string()),
        }
    };
    match outcome {
        Ok((turn, r)) => {
            shared
                .emit(Event::Undone {
                    turn,
                    reverted: r.reverted,
                    skipped: r.skipped,
                })
                .await;
        }
        Err(e) => shared.error(e, false).await,
    }
}

fn lock_tracker(
    t: &SharedTracker,
) -> std::sync::MutexGuard<'_, cosmos_llm_coding_tools::FileTracker> {
    t.lock().unwrap_or_else(|e| e.into_inner())
}

/// Runs the compactor and applies its outcome to the thread and the log.
async fn compact(
    shared: &Shared,
    o: &mut Owned,
    reason: CompactionReason,
    instructions: Option<String>,
) -> std::result::Result<(), String> {
    let Some(compactor) = &shared.compactor else {
        return Err("no compactor is installed (170)".into());
    };
    let budget = shared.budget();
    let before = lock(&shared.ctx).estimate(o.thread.history());
    let plan = CompactionPlan {
        reason,
        used: before,
        window: budget.map_or(0, |b| b.window),
        target: budget.map_or(0, |b| b.compact_threshold() / 2),
        instructions,
    };
    shared.emit(Event::CompactionStarted { reason }).await;
    let outcome = compactor
        .compact(&plan, o.thread.history())
        .await
        .map_err(|e| format!("compaction failed: {e}"))?;

    let range = outcome.replaces.clone();
    let history = o.thread.history();
    if range.start >= range.end || range.end > history.len() {
        return Err("the compactor returned an invalid range".into());
    }
    let mut candidate: Vec<Message> = history[..range.start].to_vec();
    candidate.push(outcome.summary.clone());
    candidate.extend_from_slice(&history[range.end..]);
    if let Err(e) = cosmos_llm_tool::validate_history(&candidate) {
        return Err(format!("the compacted history would be invalid: {e}"));
    }

    let replaced: Vec<MsgId> = o.sess.msg_ids[range.clone()].to_vec();
    let lo = replaced.iter().copied().min().unwrap_or(0);
    let hi = replaced.iter().copied().max().unwrap_or(0) + 1;
    let except: Vec<MsgId> = o
        .sess
        .msg_ids
        .iter()
        .copied()
        .filter(|id| *id >= lo && *id < hi && !replaced.contains(id))
        .collect();
    let summary_id = o.sess.next_id;
    o.sess.next_id += 1;
    if let Some(w) = &shared.writer {
        w.append_message(summary_id, &outcome.summary, None)
            .and_then(|()| {
                w.append(&Record::Compaction {
                    replaces: lo..hi,
                    summary: summary_id,
                    except,
                })
            })
            .map_err(|e| format!("session log: {e}"))?;
    }
    o.thread
        .history_mut()
        .splice(range.clone(), [outcome.summary]);
    o.sess.msg_ids.splice(range, [summary_id]);
    lock(&shared.ctx).invalidate();
    let after = lock(&shared.ctx).estimate(o.thread.history());
    shared
        .emit(Event::CompactionFinished { before, after })
        .await;
    shared.report_context(after).await;
    Ok(())
}

/// What the bridge hands back when the run's events are exhausted.
struct BridgeOut {
    usages: Vec<TokenUsage>,
}

/// Turns the loop's [`AgentEvent`]s into [`Event`]s.
///
/// Approval questions are held back until the interactive approver actually
/// asks: the policy answers most calls itself, and the frontend should only
/// hear about the ones it must decide.
async fn bridge(shared: Arc<Shared>, mut rx: mpsc::Receiver<AgentEvent>) -> BridgeOut {
    let mut asked_rx = shared.asked_rx.lock().await;
    let mut usages = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut asked: HashMap<String, ApprovalRequest> = HashMap::new();
    let mut announced: HashSet<String> = HashSet::new();

    async fn announce(shared: &Shared, req: ApprovalRequest) {
        shared
            .emit(Event::ApprovalRequested {
                call_id: req.call_id,
                tool: req.tool,
                input: req.input,
                preview: req.preview,
                hints: req.hints,
            })
            .await;
    }

    loop {
        tokio::select! {
            biased;
            Some(req) = asked_rx.recv() => {
                // A subagent's call is never in the parent's `seen`; its id
                // carries the `task` call and a separator instead.
                if seen.contains(&req.call_id)
                    || req.call_id.contains(crate::subagent::SUBAGENT_ID_SEP)
                {
                    announced.insert(req.call_id.clone());
                    announce(&shared, req).await;
                } else {
                    asked.insert(req.call_id.clone(), req);
                }
            }
            ev = rx.recv() => {
                let Some(ev) = ev else { break };
                match ev {
                    AgentEvent::ApprovalRequested { request } => {
                        seen.insert(request.call_id.clone());
                        if let Some(req) = asked.remove(&request.call_id) {
                            announced.insert(req.call_id.clone());
                            announce(&shared, req).await;
                        }
                    }
                    AgentEvent::ApprovalResolved { ref call_id, .. } => {
                        if announced.contains(call_id) {
                            shared.emit(Event::Agent(ev)).await;
                        }
                    }
                    AgentEvent::ToolFinished { ref call_id, ref result } => {
                        if let Some(display) = result.display.clone() {
                            let _ = shared.log(&Record::ToolOutcome {
                                call_id: call_id.clone(),
                                display: Some(display),
                            });
                        }
                        shared.emit(Event::Agent(ev)).await;
                    }
                    AgentEvent::Usage { ref turn, .. } => {
                        let used = context_tokens(turn);
                        usages.push(*turn);
                        shared.emit(Event::Agent(ev)).await;
                        shared.report_context(used).await;
                    }
                    other => shared.emit(Event::Agent(other)).await,
                }
            }
        }
    }
    BridgeOut { usages }
}

async fn run_turn(
    shared: Arc<Shared>,
    agent: Arc<AgentLoop>,
    mut o: Owned,
    blocks: Vec<cosmos_llm::ContentBlock>,
    cancel: CancellationToken,
) -> Owned {
    if let Some((tracker, _)) = &shared.tracking {
        lock_tracker(tracker).begin_turn();
    }
    let user_msg = Message::user_blocks(blocks);

    // Budget check before the model call (160).
    let mut probe: Vec<Message> = o.thread.history().to_vec();
    probe.push(user_msg.clone());
    let used = lock(&shared.ctx).estimate(&probe);
    let mut pressure = shared.report_context(used).await;
    if pressure != Pressure::Ok {
        let reason = CompactionReason::Threshold;
        if shared.compactor.is_some() && !o.thread.history().is_empty() {
            if let Err(e) = compact(&shared, &mut o, reason, None).await {
                shared.error(e, false).await;
            }
            let mut probe: Vec<Message> = o.thread.history().to_vec();
            probe.push(user_msg.clone());
            let used = lock(&shared.ctx).estimate(&probe);
            pressure = shared.report_context(used).await;
        } else if pressure == Pressure::Compact
            && !shared.warned_no_compactor.swap(true, Ordering::Relaxed)
        {
            shared
                .warn("the context window is filling up and no compactor is installed")
                .await;
        }
    }
    if pressure == Pressure::Overflow {
        shared
            .error(
                "this input does not fit in the model's context window, even after compaction",
                false,
            )
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

    o.thread.history_mut().push(user_msg);
    let mut no_usage = VecDeque::new();
    if let Some(e) = persist(&shared, &mut o.sess, o.thread.history(), &mut no_usage) {
        shared.warn(e).await;
    }

    let mut attempts = 0;
    let (result, usages) = loop {
        attempts += 1;
        let (tx, rx) = event_channel(256);
        let handles = RunHandles::new().events(tx).cancel(cancel.clone());
        let bridge_task = tokio::spawn(bridge(Arc::clone(&shared), rx));
        let result = o
            .thread
            .run_queued(&agent, shared.completer.as_ref(), &handles)
            .await;
        drop(handles);
        let usages = bridge_task.await.map_or_else(|_| Vec::new(), |b| b.usages);

        // The provider said "too long" though the estimate disagreed: compact
        // once and retry.
        if let Err(CosmosError::ContextLength { .. }) = &result {
            if attempts == 1 && shared.compactor.is_some() {
                let mut q = usages.iter().copied().collect::<VecDeque<_>>();
                let _ = persist(&shared, &mut o.sess, o.thread.history(), &mut q);
                match compact(&shared, &mut o, CompactionReason::ContextLength, None).await {
                    Ok(()) => continue,
                    Err(e) => shared.error(e, false).await,
                }
            }
        }
        break (result, usages);
    };

    // Log what the run added, then anchor the estimate on the last response.
    let mut queue: VecDeque<TokenUsage> = usages.iter().copied().collect();
    if let Some(e) = persist(&shared, &mut o.sess, o.thread.history(), &mut queue) {
        shared.warn(e).await;
    }
    if let Some(last) = usages.last() {
        if let Some(i) = o
            .thread
            .history()
            .iter()
            .rposition(|m| m.role == Role::Assistant)
        {
            lock(&shared.ctx).record_response(last, i + 1);
        }
    }

    file_changes(&shared).await;

    if let Some(w) = &shared.writer {
        if let Err(e) = w.sync().await {
            shared.warn(format!("session log: {e}")).await;
        }
    }

    match result {
        Ok(out) => {
            shared
                .emit(Event::RunFinished {
                    reason: Some(out.reason),
                    steps: out.steps_taken,
                    usage: shared.ext.rollup(out.usage),
                })
                .await;
        }
        Err(e) => {
            shared.error(e.to_string(), false).await;
            shared
                .emit(Event::RunFinished {
                    reason: None,
                    steps: 0,
                    usage: TokenUsage::default(),
                })
                .await;
        }
    }
    o
}

/// Emits the turn's diff and logs the pre-turn contents for undo (130).
async fn file_changes(shared: &Shared) {
    let Some((tracker, fs)) = &shared.tracking else {
        return;
    };
    let (turn, files, diffs) = {
        let t = lock_tracker(tracker);
        let turn = t.turn();
        let Some(changes) = t.changes(turn) else {
            return;
        };
        if changes.files.is_empty() {
            return;
        }
        let mut files = Vec::new();
        for f in &changes.files {
            let before = f.before.as_deref().and_then(|id| {
                let bytes = t.blob(id)?;
                shared
                    .writer
                    .as_ref()
                    .and_then(|w| write_blob(w.path(), bytes).ok())
            });
            files.push(FileChange::new(f.path.clone(), before, f.after.clone()));
        }
        (turn, files, t.turn_diff(fs.as_ref(), turn))
    };
    if let Some(e) = shared.log(&Record::FileChanges { turn, files }) {
        shared.warn(e).await;
    }
    shared.emit(Event::TurnDiff { files: diffs }).await;
}

/// The model metadata the core resolves for `model`, for hosts that want to
/// show it.
pub fn resolve_model_info(config: &CoreConfig, model: &str) -> Option<ModelInfo> {
    config.model_info(model)
}
