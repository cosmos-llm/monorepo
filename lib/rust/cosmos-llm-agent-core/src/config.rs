//! `CoreConfig`: the config IR, and how it is loaded (005).
//!
//! Front ends lower into [`CoreConfig`]: `.trongo` files evaluate to a value
//! under the sandbox in [`eval`](crate::eval); `config.toml` and `config.json`
//! are read directly; both end in `serde_json::from_value`. The serde shape is
//! the config schema.
//!
//! Two layers load, user then project:
//!
//! - `~/.config/cosmos/config.{trongo,toml,json}` (`$XDG_CONFIG_HOME/cosmos`)
//! - `<repo>/.cosmos/config.{trongo,toml,json}`
//!
//! The project layer comes from a cloned repository, so by default it is
//! **untrusted**: it may add deny rules and tighten the mode, but its allow
//! rules, `shell_tools`, `edit_tools` and `sessions.dir` are dropped with a
//! warning. [`LoadOptions::trust_project`] lifts that.

use std::path::{Path, PathBuf};

use cosmos_llm::{ModelInfo, Reasoning};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::compaction::CompactionConfig;
use crate::error::{CoreError, Result};
use crate::eval::{ConfigEvaluator, EvalSnapshot};
use crate::policy::{PolicySpec, Rule};

/// Limits on what the context window may hold.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct ContextConfig {
    /// Overrides the model's context window.
    pub window: Option<u32>,
    /// Tokens kept free for the reply. Defaults to the model's output cap,
    /// at most 32,000.
    pub reserve_output: Option<u32>,
    /// Fraction of `window - reserve_output` at which compaction triggers.
    pub compact_at: f64,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            window: None,
            reserve_output: None,
            compact_at: 0.85,
        }
    }
}

/// Session log settings (200).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct SessionConfig {
    /// Whether to write a session log at all.
    pub enabled: bool,
    /// The sessions root. Defaults to `$XDG_DATA_HOME/cosmos/sessions`.
    pub dir: Option<PathBuf>,
    /// Sessions older than this many days are deleted at startup.
    pub retention_days: Option<u32>,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            dir: None,
            retention_days: None,
        }
    }
}

/// Everything the core reads from config.
///
/// Live resources (the client, the tools, the shell parser) are not config;
/// they go to [`CoreBuilder`](crate::CoreBuilder).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct CoreConfig {
    /// The model to start with.
    pub model: Option<String>,
    /// The workspace. Defaults to the process's current directory.
    pub cwd: Option<PathBuf>,
    /// Replaces the built-in base system prompt.
    pub system_prompt: Option<String>,
    /// Whether to load `AGENTS.md`/`CLAUDE.md` and environment facts into the
    /// system prompt (190).
    pub instructions: bool,
    /// Model turns allowed per user input.
    pub max_steps: u32,
    /// Reasoning to request, if the model supports it.
    pub reasoning: Option<Reasoning>,
    /// Approval policy (150).
    pub permissions: PolicySpec,
    /// Context window handling (160).
    pub context: ContextConfig,
    /// Compaction (170).
    pub compaction: CompactionConfig,
    /// Session persistence (200).
    pub sessions: SessionConfig,
    /// Model metadata overrides (050), by id.
    pub models: Vec<ModelInfo>,
    /// Capacity of the event channel to the frontend.
    pub event_buffer: usize,
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            model: None,
            cwd: None,
            system_prompt: None,
            instructions: true,
            max_steps: 50,
            reasoning: None,
            permissions: PolicySpec::default(),
            context: ContextConfig::default(),
            compaction: CompactionConfig::default(),
            sessions: SessionConfig::default(),
            models: Vec::new(),
            event_buffer: 1024,
        }
    }
}

impl CoreConfig {
    /// Lowers an evaluated value into the IR.
    pub fn from_value(value: Value) -> Result<Self> {
        serde_json::from_value(value)
            .map_err(|e| CoreError::Config(format!("value does not match CoreConfig: {e}")))
    }

    /// Evaluates TrongoScript `source` (no filesystem access) into a config.
    pub fn from_trongo_str(source: &str) -> Result<Self> {
        let ev = ConfigEvaluator::new(EvalSnapshot::empty());
        Self::from_value(ev.eval_source(source, None)?)
    }

    /// Reads a TOML config.
    pub fn from_toml_str(source: &str) -> Result<Self> {
        Self::from_value(parse_toml(source)?)
    }

    /// Loads and merges the user and project layers. See the
    /// [module docs](self).
    pub fn load(opts: &LoadOptions) -> Result<LoadedConfig> {
        load_config(opts)
    }

    /// The model metadata for `model`: a `models` override if there is one,
    /// else the built-in table.
    pub fn model_info(&self, model: &str) -> Option<ModelInfo> {
        self.models
            .iter()
            .find(|m| m.id == model)
            .cloned()
            .or_else(|| ModelInfo::builtin(model))
    }
}

fn parse_toml(source: &str) -> Result<Value> {
    toml::from_str::<Value>(source).map_err(|e| CoreError::Config(format!("toml: {e}")))
}

/// Which syntax a config file is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ConfigFormat {
    /// `config.trongo`
    Trongo,
    /// `config.toml`
    Toml,
    /// `config.json`
    Json,
}

/// Which layer a file came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Layer {
    /// `~/.config/cosmos`
    User,
    /// `<repo>/.cosmos`
    Project,
}

/// A file that contributed to the loaded config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ConfigSource {
    /// The file.
    pub path: PathBuf,
    /// The layer.
    pub layer: Layer,
    /// The syntax.
    pub format: ConfigFormat,
    /// Whether the file was allowed to widen permissions.
    pub trusted: bool,
}

/// The result of [`CoreConfig::load`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct LoadedConfig {
    /// The merged config.
    pub config: CoreConfig,
    /// The files read, user first.
    pub sources: Vec<ConfigSource>,
    /// Things worth showing the user: dropped untrusted settings, two config
    /// files in one directory.
    pub warnings: Vec<String>,
}

/// Where to look and how far to trust it.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct LoadOptions {
    /// The workspace directory.
    pub cwd: PathBuf,
    /// The user config directory. `None` resolves `$XDG_CONFIG_HOME/cosmos`
    /// or `~/.config/cosmos`.
    pub user_dir: Option<PathBuf>,
    /// The project config directory. `None` is `<git root or cwd>/.cosmos`.
    pub project_dir: Option<PathBuf>,
    /// Whether the project layer may widen permissions.
    pub trust_project: bool,
    /// The world host builtins see. `None` captures it.
    pub snapshot: Option<EvalSnapshot>,
}

impl LoadOptions {
    /// Defaults for a workspace at `cwd`: standard directories, untrusted
    /// project layer.
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            user_dir: None,
            project_dir: None,
            trust_project: false,
            snapshot: None,
        }
    }

    /// Sets the user config directory.
    #[must_use]
    pub fn user_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.user_dir = Some(dir.into());
        self
    }

    /// Sets the project config directory.
    #[must_use]
    pub fn project_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.project_dir = Some(dir.into());
        self
    }

    /// Lets the project layer widen permissions.
    #[must_use]
    pub fn trust_project(mut self, yes: bool) -> Self {
        self.trust_project = yes;
        self
    }

    /// Fixes the world host builtins see.
    #[must_use]
    pub fn snapshot(mut self, snapshot: EvalSnapshot) -> Self {
        self.snapshot = Some(snapshot);
        self
    }

    /// The user config directory this would read.
    pub fn resolved_user_dir(&self) -> Option<PathBuf> {
        if let Some(d) = &self.user_dir {
            return Some(d.clone());
        }
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|v| !v.is_empty())
                    .map(|h| PathBuf::from(h).join(".config"))
            })?;
        Some(base.join("cosmos"))
    }

    /// The project config directory this would read.
    pub fn resolved_project_dir(&self) -> PathBuf {
        self.project_dir.clone().unwrap_or_else(|| {
            cosmos_llm_context::find_git_root(&self.cwd)
                .unwrap_or_else(|| self.cwd.clone())
                .join(".cosmos")
        })
    }
}

const FILE_NAMES: &[(&str, ConfigFormat)] = &[
    ("config.trongo", ConfigFormat::Trongo),
    ("config.toml", ConfigFormat::Toml),
    ("config.json", ConfigFormat::Json),
];

fn find_config(dir: &Path, warnings: &mut Vec<String>) -> Option<(&'static str, ConfigFormat)> {
    let found: Vec<_> = FILE_NAMES
        .iter()
        .filter(|(n, _)| dir.join(n).is_file())
        .collect();
    if found.len() > 1 {
        warnings.push(format!(
            "{} holds several config files; using {}",
            dir.display(),
            found[0].0
        ));
    }
    found.first().map(|(n, f)| (*n, *f))
}

fn read_layer(ev: &ConfigEvaluator, dir: &Path, name: &str, format: ConfigFormat) -> Result<Value> {
    match format {
        ConfigFormat::Trongo => ev.eval_file(dir, name),
        ConfigFormat::Toml | ConfigFormat::Json => {
            let path = dir.join(name);
            let meta = std::fs::metadata(&path).map_err(|e| CoreError::io(&path, e))?;
            if meta.len() > 1024 * 1024 {
                return Err(CoreError::Config(format!(
                    "{} is over 1 MiB",
                    path.display()
                )));
            }
            let text = std::fs::read_to_string(&path).map_err(|e| CoreError::io(&path, e))?;
            if format == ConfigFormat::Toml {
                parse_toml(&text)
            } else {
                serde_json::from_str(&text).map_err(Into::into)
            }
        }
    }
}

/// Deep-merges `over` into `base`. Objects merge, other values replace, except
/// `permissions.allow` and `permissions.deny`, which concatenate.
pub fn merge_values(base: &mut Value, over: Value) {
    merge_at(base, over, &mut Vec::new());
}

fn merge_at(base: &mut Value, over: Value, path: &mut Vec<String>) {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                path.push(k.clone());
                match b.get_mut(&k) {
                    Some(slot) => merge_at(slot, v, path),
                    None => {
                        b.insert(k, v);
                    }
                }
                path.pop();
            }
        }
        (Value::Array(b), Value::Array(o))
            if matches!(
                path.iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .as_slice(),
                ["permissions", "allow"] | ["permissions", "deny"]
            ) =>
        {
            b.extend(o);
        }
        (slot, v) => *slot = v,
    }
}

fn mode_of(v: &Value) -> crate::policy::ApprovalMode {
    v.pointer("/permissions/mode")
        .and_then(|m| serde_json::from_value(m.clone()).ok())
        .unwrap_or_default()
}

/// Removes what an untrusted layer may not set. Returns warnings.
fn restrict_untrusted(layer: &mut Value, merged_so_far: &Value, name: &str) -> Vec<String> {
    let mut warnings = Vec::new();
    let Some(obj) = layer.as_object_mut() else {
        return warnings;
    };
    if let Some(Value::Object(perm)) = obj.get_mut("permissions") {
        for key in ["allow", "shell_tools", "edit_tools"] {
            if perm.remove(key).is_some() {
                warnings.push(format!(
                    "{name}: ignored permissions.{key} from an untrusted project config"
                ));
            }
        }
        if let Some(mode) = perm.get("mode") {
            let wanted: crate::policy::ApprovalMode =
                serde_json::from_value(mode.clone()).unwrap_or_default();
            let current = mode_of(merged_so_far);
            let allowed = wanted.stricter(current);
            if allowed != wanted {
                warnings.push(format!(
                    "{name}: permissions.mode limited to {allowed:?} (untrusted project config)"
                ));
                perm.insert(
                    "mode".into(),
                    serde_json::to_value(allowed).unwrap_or(Value::Null),
                );
            }
        }
    }
    if let Some(Value::Object(s)) = obj.get_mut("sessions") {
        if s.remove("dir").is_some() {
            warnings.push(format!(
                "{name}: ignored sessions.dir from an untrusted project config"
            ));
        }
    }
    if obj.remove("cwd").is_some() {
        warnings.push(format!(
            "{name}: ignored cwd from an untrusted project config"
        ));
    }
    warnings
}

fn load_config(opts: &LoadOptions) -> Result<LoadedConfig> {
    let snapshot = opts
        .snapshot
        .clone()
        .unwrap_or_else(|| EvalSnapshot::capture(&opts.cwd));
    let user_dir = opts.resolved_user_dir();
    let project_dir = opts.resolved_project_dir();

    let mut evaluator = ConfigEvaluator::new(snapshot);
    if let Some(u) = &user_dir {
        if u.is_dir() {
            evaluator = evaluator.with_mount("/user", u);
        }
    }

    let mut warnings = Vec::new();
    let mut sources = Vec::new();
    let mut merged = Value::Object(serde_json::Map::new());

    let layers = [(Layer::User, user_dir), (Layer::Project, Some(project_dir))];
    for (layer, dir) in layers {
        let Some(dir) = dir.filter(|d| d.is_dir()) else {
            continue;
        };
        let Some((name, format)) = find_config(&dir, &mut warnings) else {
            continue;
        };
        let path = dir.join(name);
        let mut value = read_layer(&evaluator, &dir, name, format)
            .map_err(|e| CoreError::Config(format!("{}: {e}", path.display())))?;
        if !value.is_object() && !value.is_null() {
            return Err(CoreError::Config(format!(
                "{}: config must evaluate to a map",
                path.display()
            )));
        }
        if value.is_null() {
            value = Value::Object(serde_json::Map::new());
        }
        let trusted = layer == Layer::User || opts.trust_project;
        if !trusted {
            warnings.extend(restrict_untrusted(
                &mut value,
                &merged,
                &path.display().to_string(),
            ));
        }
        merge_values(&mut merged, value);
        sources.push(ConfigSource {
            path,
            layer,
            format,
            trusted,
        });
    }

    Ok(LoadedConfig {
        config: CoreConfig::from_value(merged)?,
        sources,
        warnings,
    })
}

/// Appends an allow rule to `<dir>/permissions.trongo`, printed as TrongoScript
/// data. The main config pulls it in with `(include "permissions.trongo")`.
/// Returns the file written.
pub fn append_permission_rule(dir: &Path, rule: &Rule) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).map_err(|e| CoreError::io(dir, e))?;
    let path = dir.join("permissions.trongo");
    let mut rules: Vec<Rule> = if path.is_file() {
        let ev = ConfigEvaluator::new(EvalSnapshot::empty());
        let v = ev.eval_file(dir, "permissions.trongo")?;
        serde_json::from_value(v)
            .map_err(|e| CoreError::Config(format!("{}: {e}", path.display())))?
    } else {
        Vec::new()
    };
    if !rules.contains(rule) {
        rules.push(rule.clone());
    }
    let text = trongoscript::to_trongo_source_pretty(&serde_json::to_value(&rules)?);
    std::fs::write(&path, format!("{text}\n")).map_err(|e| CoreError::io(&path, e))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::ApprovalMode;
    use serde_json::json;

    fn write(dir: &Path, name: &str, text: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(name), text).unwrap();
    }

    fn opts(root: &Path) -> LoadOptions {
        LoadOptions::new(root)
            .user_dir(root.join("user"))
            .project_dir(root.join("proj"))
            .snapshot(EvalSnapshot::empty().with_env("CI", "true"))
    }

    #[test]
    fn defaults() {
        let c = CoreConfig::from_value(json!({})).unwrap();
        assert_eq!(c, CoreConfig::default());
        assert_eq!(c.permissions.mode, ApprovalMode::Ask);
    }

    #[test]
    fn permissions_example_from_150() {
        let src = r#"
(def cargo-safe ["cargo test*" "cargo check*" "cargo clippy*"])
(def cfg
{:permissions
 {:mode  (if (= (env "CI") "true") :read-only :ask)
  :allow (concat (map (fn [c] {:tool :bash :command c}) cargo-safe)
                 [{:tool :edit :path "docs/**"}])
  :deny  [{:tool :bash :command "git push*"}]}})
cfg"#;
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("user"), "config.trongo", src);
        let loaded = CoreConfig::load(&opts(dir.path())).unwrap();
        let p = loaded.config.permissions;
        assert_eq!(p.mode, ApprovalMode::ReadOnly);
        assert_eq!(p.allow.len(), 4);
        assert_eq!(p.allow[0], Rule::tool("bash").command("cargo test*"));
        assert_eq!(p.allow[3], Rule::tool("edit").path("docs/**"));
        assert_eq!(p.deny, vec![Rule::tool("bash").command("git push*")]);
    }

    #[test]
    fn toml_loads_into_the_same_ir() {
        let c = CoreConfig::from_toml_str(
            r#"
model = "claude-sonnet-4"
max_steps = 10
[permissions]
mode = "auto_edit"
deny = [{ tool = "bash", command = "git push*" }]
[context]
compact_at = 0.5
"#,
        )
        .unwrap();
        assert_eq!(c.model.as_deref(), Some("claude-sonnet-4"));
        assert_eq!(c.permissions.mode, ApprovalMode::AutoEdit);
        assert_eq!(c.context.compact_at, 0.5);
    }

    #[test]
    fn layers_merge_and_rules_concatenate() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("user"),
            "config.toml",
            "model = \"a\"\n[permissions]\nmode = \"ask\"\nallow = [{tool = \"read\"}]\n",
        );
        write(
            &dir.path().join("proj"),
            "config.trongo",
            r#"{:model "b" :permissions {:deny [{:tool :bash :command "rm*"}]}}"#,
        );
        let l = CoreConfig::load(&opts(dir.path())).unwrap();
        assert_eq!(l.config.model.as_deref(), Some("b"));
        assert_eq!(l.config.permissions.allow.len(), 1);
        assert_eq!(l.config.permissions.deny.len(), 1);
        assert_eq!(l.sources.len(), 2);
        assert!(l.warnings.is_empty());
    }

    #[test]
    fn untrusted_project_cannot_widen() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("user"),
            "config.toml",
            "[permissions]\nmode = \"ask\"\n",
        );
        write(
            &dir.path().join("proj"),
            "config.trongo",
            r#"{:permissions {:mode :full-auto :allow [{:tool :bash}]} :sessions {:dir "/etc"}}"#,
        );
        let l = CoreConfig::load(&opts(dir.path())).unwrap();
        assert_eq!(l.config.permissions.mode, ApprovalMode::Ask);
        assert!(l.config.permissions.allow.is_empty());
        assert!(l.config.sessions.dir.is_none());
        assert_eq!(l.warnings.len(), 3);

        let t = CoreConfig::load(&opts(dir.path()).trust_project(true)).unwrap();
        assert_eq!(t.config.permissions.mode, ApprovalMode::FullAuto);
        assert_eq!(t.config.permissions.allow.len(), 1);
    }

    #[test]
    fn project_layer_may_tighten() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("user"),
            "config.toml",
            "[permissions]\nmode = \"auto_edit\"\n",
        );
        write(
            &dir.path().join("proj"),
            "config.toml",
            "[permissions]\nmode = \"read_only\"\n",
        );
        let l = CoreConfig::load(&opts(dir.path())).unwrap();
        assert_eq!(l.config.permissions.mode, ApprovalMode::ReadOnly);
        assert!(l.warnings.is_empty());
    }

    #[test]
    fn remembered_rules_round_trip_through_include() {
        let dir = tempfile::tempdir().unwrap();
        let proj = dir.path().join("proj");
        let r1 = Rule::tool("bash").command("make test");
        append_permission_rule(&proj, &r1).unwrap();
        append_permission_rule(&proj, &r1).unwrap();
        append_permission_rule(&proj, &Rule::tool("edit").path("src/**")).unwrap();
        write(
            &proj,
            "config.trongo",
            r#"{:permissions {:allow (include "permissions.trongo")}}"#,
        );
        let l = CoreConfig::load(&opts(dir.path()).trust_project(true)).unwrap();
        assert_eq!(
            l.config.permissions.allow,
            vec![r1, Rule::tool("edit").path("src/**")]
        );
    }

    #[test]
    fn nonsense_is_a_config_error() {
        assert!(matches!(
            CoreConfig::from_value(json!({"max_steps": "many"})),
            Err(CoreError::Config(_))
        ));
        assert!(CoreConfig::from_trongo_str("(boom").is_err());
    }
}
