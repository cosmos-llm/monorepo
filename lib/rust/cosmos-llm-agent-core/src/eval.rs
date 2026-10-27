//! TrongoScript evaluation for config (005): the sandbox, the limits, and the
//! host builtins.
//!
//! Evaluating `<repo>/.cosmos/*.trongo` from a cloned repository is only safe
//! because the language has no ambient capability. This module keeps it so:
//!
//! - The sandbox mounts one directory read-only (the config directory being
//!   loaded), never `/` and never the repo.
//! - Host builtins are pure lookups into an [`EvalSnapshot`] taken before
//!   evaluation starts. None runs a command, writes a file or opens a socket.
//! - Evaluation is metered ([`trongoscript::FuelLimits`]) and runs on its own
//!   thread under a wall-clock timeout, so a hostile file cannot hang startup.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use trongoscript::{
    eval_source, BuiltinError, Context, EvalConfig, FuelLimits, HostBuiltin, Sandbox, SandboxLimits,
};

use crate::error::{CoreError, Result};

/// Environment variables config may read, besides `COSMOS_*`.
pub const ENV_ALLOWLIST: &[&str] = &["CI", "HOME", "USER", "TERM"];

const SECRET_MARKERS: &[&str] = &["KEY", "TOKEN", "SECRET", "PASSWORD", "PASSWD", "CREDENTIAL"];

/// Whether config may see the variable `name`.
///
/// Allowlisted names and `COSMOS_*` are visible, unless the name looks like a
/// secret (`*_API_KEY`, `*_TOKEN`, ...), which is never visible.
pub fn env_visible(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    if SECRET_MARKERS.iter().any(|m| upper.contains(m)) {
        return false;
    }
    ENV_ALLOWLIST.contains(&name) || name.starts_with("COSMOS_")
}

/// What `(platform)` returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Platform {
    /// `linux`, `macos`, ...
    pub os: String,
    /// `x86_64`, `aarch64`, ...
    pub arch: String,
    /// The login shell's name (`bash`), or empty.
    pub shell: String,
}

impl Platform {
    /// The platform this process runs on.
    pub fn current() -> Self {
        let shell = std::env::var("SHELL")
            .ok()
            .and_then(|s| {
                Path::new(&s)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
            })
            .unwrap_or_default();
        Self {
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            shell,
        }
    }
}

/// Everything the host builtins can see, gathered before evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct EvalSnapshot {
    /// Visible environment variables.
    pub env: BTreeMap<String, String>,
    /// The checked-out branch, if the cwd is in a repository on a branch.
    pub git_branch: Option<String>,
    /// The repository root.
    pub git_root: Option<String>,
    /// The platform.
    pub platform: Platform,
}

impl EvalSnapshot {
    /// An empty snapshot, for tests and for hosts that want a fixed world.
    pub fn empty() -> Self {
        Self {
            env: BTreeMap::new(),
            git_branch: None,
            git_root: None,
            platform: Platform::current(),
        }
    }

    /// Reads the process environment and the repository around `cwd`.
    ///
    /// Git state comes from reading `.git/HEAD`; no subprocess is spawned.
    pub fn capture(cwd: &Path) -> Self {
        let env = std::env::vars()
            .filter(|(k, _)| env_visible(k))
            .collect::<BTreeMap<_, _>>();
        let root = cosmos_llm_context::find_git_root(cwd);
        let branch = root.as_deref().and_then(read_branch);
        Self {
            env,
            git_branch: branch,
            git_root: root.map(|r| r.to_string_lossy().into_owned()),
            platform: Platform::current(),
        }
    }

    /// Adds an environment variable (subject to [`env_visible`]).
    #[must_use]
    pub fn with_env(mut self, name: &str, value: &str) -> Self {
        if env_visible(name) {
            self.env.insert(name.to_owned(), value.to_owned());
        }
        self
    }

    /// Sets the git facts.
    #[must_use]
    pub fn with_git(mut self, root: Option<&str>, branch: Option<&str>) -> Self {
        self.git_root = root.map(str::to_owned);
        self.git_branch = branch.map(str::to_owned);
        self
    }
}

fn read_branch(root: &Path) -> Option<String> {
    let dot_git = root.join(".git");
    let git_dir = if dot_git.is_file() {
        let text = std::fs::read_to_string(&dot_git).ok()?;
        let rel = text.trim().strip_prefix("gitdir:")?.trim();
        let p = Path::new(rel);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            root.join(p)
        }
    } else {
        dot_git
    };
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    head.trim()
        .strip_prefix("ref: refs/heads/")
        .map(str::to_owned)
}

/// Evaluates TrongoScript config under the sandbox rules above.
#[derive(Debug, Clone)]
pub struct ConfigEvaluator {
    snapshot: Arc<EvalSnapshot>,
    fuel: FuelLimits,
    sandbox_limits: SandboxLimits,
    timeout: Duration,
    mounts: Vec<(String, PathBuf)>,
}

impl ConfigEvaluator {
    /// An evaluator over `snapshot` with the default limits: 2M steps, 1 MiB
    /// per file, 5 seconds.
    pub fn new(snapshot: EvalSnapshot) -> Self {
        Self {
            snapshot: Arc::new(snapshot),
            fuel: FuelLimits {
                max_steps: 2_000_000,
                max_call_depth: 256,
                max_eval_depth: 1_024,
                max_value_len: 1024 * 1024,
                max_total_alloc: 32 * 1024 * 1024,
            },
            sandbox_limits: SandboxLimits {
                max_file_bytes: 1024 * 1024,
                ..SandboxLimits::default()
            },
            timeout: Duration::from_secs(5),
            mounts: Vec::new(),
        }
    }

    /// Sets the wall-clock limit.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Sets the interpreter's step budget.
    #[must_use]
    pub fn with_max_steps(mut self, steps: u64) -> Self {
        self.fuel.max_steps = steps;
        self
    }

    /// Mounts another read-only directory at the virtual path `at`
    /// (`/user`), for config that includes from `~/.config/cosmos`.
    #[must_use]
    pub fn with_mount(mut self, at: &str, dir: impl Into<PathBuf>) -> Self {
        self.mounts.push((at.to_owned(), dir.into()));
        self
    }

    /// Evaluates the file `name` inside `root`. The sandbox is `root`.
    pub fn eval_file(&self, root: &Path, name: &str) -> Result<Value> {
        self.run(Some(root.to_path_buf()), Source::File(name.to_owned()))
    }

    /// Evaluates `source`. With a `root`, `include` and `read-file` work
    /// inside it; without one they fail.
    pub fn eval_source(&self, source: &str, root: Option<&Path>) -> Result<Value> {
        self.run(root.map(Path::to_path_buf), Source::Text(source.to_owned()))
    }

    fn run(&self, root: Option<PathBuf>, source: Source) -> Result<Value> {
        let this = self.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("cosmos-config-eval".into())
            .stack_size(64 * 1024 * 1024)
            .spawn(move || {
                let _ = tx.send(this.run_inner(root.as_deref(), source));
            })
            .map_err(|e| CoreError::Eval(format!("could not start evaluator: {e}")))?;
        match rx.recv_timeout(self.timeout) {
            Ok(result) => result,
            Err(_) => Err(CoreError::Eval(format!(
                "evaluation exceeded {:?}",
                self.timeout
            ))),
        }
    }

    fn run_inner(&self, root: Option<&Path>, source: Source) -> Result<Value> {
        let mut cfg = EvalConfig::new().with_fuel_limits(self.fuel);
        let mut sandbox = None;
        if let Some(root) = root {
            let mut sb = Sandbox::new(root)
                .map_err(|e| CoreError::Eval(format!("{}: {e}", root.display())))?
                .with_limits(self.sandbox_limits.clone());
            for (at, dir) in &self.mounts {
                if dir.is_dir() {
                    sb = sb
                        .mount(at, dir)
                        .map_err(|e| CoreError::Eval(format!("mount {at}: {e}")))?;
                }
            }
            sandbox = Some(sb);
        }
        if let Some(sb) = &sandbox {
            cfg = cfg.with_sandbox(sb.clone());
        }
        for (name, builtin) in host_builtins(&self.snapshot) {
            cfg = cfg.with_host_builtin(name, builtin);
        }
        let mut ctx = Context::new(cfg);
        let text = match source {
            Source::Text(t) => t,
            Source::File(name) => {
                let sb = sandbox
                    .as_ref()
                    .ok_or_else(|| CoreError::Eval("no sandbox root".into()))?;
                sb.read_text(None, &name)
                    .map_err(|e| CoreError::Eval(format!("{name}: {e}")))?
            }
        };
        ctx.current_file_dir = sandbox.as_ref().map(|sb| sb.root().to_path_buf());
        eval_source(&text, &mut ctx).map_err(|e| CoreError::Eval(e.to_string()))
    }
}

enum Source {
    Text(String),
    File(String),
}

fn arg_str<'a>(f: &str, args: &'a [Value], i: usize) -> std::result::Result<&'a str, BuiltinError> {
    args.get(i)
        .and_then(Value::as_str)
        .ok_or_else(|| BuiltinError::ArgError {
            function: f.to_owned(),
            message: format!("argument {} must be a string", i + 1),
        })
}

fn host_builtins(snap: &Arc<EvalSnapshot>) -> Vec<(&'static str, HostBuiltin)> {
    let s = Arc::clone(snap);
    let env: HostBuiltin = Arc::new(move |_ctx, args| {
        let name = arg_str("env", args, 0)?;
        Ok(match s.env.get(name) {
            Some(v) if env_visible(name) => Value::String(v.clone()),
            _ => Value::Null,
        })
    });

    let s = Arc::clone(snap);
    let git: HostBuiltin = Arc::new(move |_ctx, args| {
        let what = arg_str("git", args, 0)?.trim_start_matches(':');
        let v = match what {
            "branch" => s.git_branch.clone(),
            "root" => s.git_root.clone(),
            other => {
                return Err(BuiltinError::ArgError {
                    function: "git".into(),
                    message: format!("unknown query `{other}` (use :branch or :root)"),
                })
            }
        };
        Ok(v.map_or(Value::Null, Value::String))
    });

    let s = Arc::clone(snap);
    let platform: HostBuiltin = Arc::new(move |_ctx, _args| {
        serde_json::to_value(&s.platform).map_err(|e| BuiltinError::Failed {
            function: "platform".into(),
            message: e.to_string(),
        })
    });

    let model_info: HostBuiltin = Arc::new(|_ctx, args| {
        let id = arg_str("model-info", args, 0)?;
        match cosmos_llm::ModelInfo::builtin(id) {
            Some(info) => serde_json::to_value(info).map_err(|e| BuiltinError::Failed {
                function: "model-info".into(),
                message: e.to_string(),
            }),
            None => Ok(Value::Null),
        }
    });

    let render: HostBuiltin = Arc::new(|ctx, args| {
        let file = arg_str("render", args, 0)?;
        let sb = ctx.sandbox.clone().ok_or_else(|| BuiltinError::Failed {
            function: "render".into(),
            message: "no sandbox".into(),
        })?;
        let source = sb
            .read_text(ctx.current_file_dir.as_deref(), file)
            .map_err(|e| BuiltinError::Failed {
                function: "render".into(),
                message: e.to_string(),
            })?;
        let mut data: HashMap<String, Value> = HashMap::new();
        match args.get(1) {
            Some(Value::Object(map)) => {
                data.extend(map.iter().map(|(k, v)| (k.clone(), v.clone())));
            }
            Some(Value::Null) | None => {}
            Some(_) => {
                return Err(BuiltinError::ArgError {
                    function: "render".into(),
                    message: "argument 2 must be a map".into(),
                })
            }
        }
        // The template runs in a fresh context: only `data` is in scope, and
        // it keeps the same sandbox but none of the host builtins.
        let mut sub = Context::new(EvalConfig::new().with_sandbox(sb)).with_data(data);
        sub.current_file_dir = ctx.current_file_dir.clone();
        trongoscript::render_template(&source, &mut sub)
            .map(Value::String)
            .map_err(|e| BuiltinError::Failed {
                function: "render".into(),
                message: e.to_string(),
            })
    });

    vec![
        ("env", env),
        ("git", git),
        ("platform", platform),
        ("model-info", model_info),
        ("render", render),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn eval(src: &str) -> Result<Value> {
        let snap = EvalSnapshot::empty()
            .with_env("CI", "true")
            .with_env("OPENAI_API_KEY", "sk-secret")
            .with_git(Some("/repo"), Some("main"));
        ConfigEvaluator::new(snap).eval_source(src, None)
    }

    #[test]
    fn env_allowlist_hides_secrets() {
        assert!(env_visible("CI"));
        assert!(env_visible("COSMOS_MODEL"));
        assert!(!env_visible("COSMOS_API_KEY"));
        assert!(!env_visible("OPENAI_API_KEY"));
        assert!(!env_visible("PATH"));
        assert_eq!(eval(r#"(env "CI")"#).unwrap(), json!("true"));
        assert_eq!(eval(r#"(env "OPENAI_API_KEY")"#).unwrap(), Value::Null);
        assert_eq!(eval(r#"(env "NOPE")"#).unwrap(), Value::Null);
    }

    #[test]
    fn git_and_platform() {
        assert_eq!(eval("(git :branch)").unwrap(), json!("main"));
        assert_eq!(eval("(git :root)").unwrap(), json!("/repo"));
        let p = eval("(platform)").unwrap();
        assert_eq!(p["os"], json!(std::env::consts::OS));
    }

    #[test]
    fn model_info_looks_up_the_table() {
        assert_eq!(
            eval(r#"(model-info "no-such-model-xyz")"#).unwrap(),
            Value::Null
        );
    }

    #[test]
    fn keywords_and_maps_become_data() {
        let v =
            eval(r#"{:permissions {:mode (if (= (env "CI") "true") :read_only :ask)}}"#).unwrap();
        assert_eq!(v, json!({"permissions": {"mode": "read_only"}}));
    }

    #[test]
    fn no_sandbox_means_no_files() {
        assert!(eval(r#"(read-file "x")"#).is_err());
    }

    #[test]
    fn include_and_render_stay_in_the_sandbox() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.trongo"), r#"{:x (include "b.trongo")}"#).unwrap();
        std::fs::write(dir.path().join("b.trongo"), "[1 2]").unwrap();
        std::fs::write(dir.path().join("t.erb"), "hi <%= name %>").unwrap();
        std::fs::write(
            dir.path().join("r.trongo"),
            r#"(render "t.erb" {:name "bob"})"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("evil.trongo"),
            r#"(read-file "../../etc/passwd")"#,
        )
        .unwrap();
        let ev = ConfigEvaluator::new(EvalSnapshot::empty());
        assert_eq!(
            ev.eval_file(dir.path(), "a.trongo").unwrap(),
            json!({"x": [1, 2]})
        );
        assert_eq!(
            ev.eval_file(dir.path(), "r.trongo").unwrap(),
            json!("hi bob")
        );
        assert!(ev.eval_file(dir.path(), "evil.trongo").is_err());
    }

    #[test]
    fn runaway_scripts_are_stopped() {
        let ev = ConfigEvaluator::new(EvalSnapshot::empty()).with_max_steps(10_000);
        let r = ev.eval_source("(defn f [] (f)) (f)", None);
        assert!(r.is_err());
    }
}
