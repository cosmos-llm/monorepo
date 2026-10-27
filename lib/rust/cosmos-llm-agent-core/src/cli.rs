//! The headless command line (220): `exec [OPTIONS] [PROMPT]`.
//!
//! [`exec_main`] parses `std::env::args`, builds a core over the coding tools
//! and a [`Client`](cosmos_llm::Client), runs one prompt with
//! [`run_exec`](crate::run_exec) and maps the result to an exit code. A host
//! binary mounts it as a subcommand:
//!
//! ```ignore
//! fn main() -> std::process::ExitCode {
//!     cosmos_llm_agent_core::cli::exec_main()
//! }
//! ```
//!
//! Exit codes: `0` finished, `1` error, `2` stopped by `--max-turns` or
//! `--max-cost`, `130` interrupted. Use [`exec_main_from`] to pass the
//! arguments after the subcommand yourself.

use std::ffi::OsString;
use std::io::{IsTerminal, Read};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::Parser;
use cosmos_llm::{Client, Config};
use cosmos_llm_coding_tools::{CodingTools, DiskFs};
use tokio_util::sync::CancellationToken;

use crate::{
    run_exec, ApprovalMode, Core, CoreConfig, ExecOptions, LoadOptions, OutputFormat, ResumeFrom,
};

/// Run one prompt through the agent without a terminal UI.
#[derive(Debug, Parser)]
#[command(name = "exec", version, about, long_about = None)]
#[command(after_help = "\
Nothing can be asked in this mode: a call the policy would ask about is denied,
with a reason the model sees, and the run goes on.

Exit codes: 0 finished, 1 error, 2 stopped by --max-turns or --max-cost, 130 interrupted.

Examples:
  exec 'fix the failing test' --mode auto-edit
  git diff | exec 'review this' --mode read-only --output jsonl")]
pub struct ExecArgs {
    /// The prompt; read from stdin when omitted or `-`.
    pub prompt: Option<String>,
    /// The model id (defaults to the config's).
    #[arg(long)]
    pub model: Option<String>,
    /// The provider: openai, anthropic, openrouter, ... (guessed from the
    /// model, or `COSMOS_PROVIDER`, when omitted).
    #[arg(long)]
    pub provider: Option<String>,
    /// Approval mode. There is no `ask`: nobody is there to answer.
    #[arg(long, value_parser = parse_mode)]
    pub mode: Option<ApprovalMode>,
    /// What to print: text, json, jsonl or trongo.
    #[arg(long, default_value = "text")]
    pub output: OutputFormat,
    /// Model turns allowed before stopping with exit code 2.
    #[arg(long)]
    pub max_turns: Option<u32>,
    /// Dollars to spend before stopping with exit code 2.
    #[arg(long)]
    pub max_cost: Option<f64>,
    /// Resume the session with this id.
    #[arg(long, conflicts_with = "continue_session")]
    pub resume: Option<String>,
    /// Continue the latest session in this directory.
    #[arg(long = "continue")]
    pub continue_session: bool,
    /// The workspace (defaults to the current directory).
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    /// Do not write a session log.
    #[arg(long)]
    pub ephemeral: bool,
}

fn parse_mode(s: &str) -> Result<ApprovalMode, String> {
    match s {
        "read-only" | "read_only" => Ok(ApprovalMode::ReadOnly),
        "auto-edit" | "auto_edit" => Ok(ApprovalMode::AutoEdit),
        "full-auto" | "full_auto" => Ok(ApprovalMode::FullAuto),
        "ask" => Err("`ask` needs someone to answer; use read-only, auto-edit or full-auto".into()),
        other => Err(format!(
            "unknown mode '{other}' (read-only, auto-edit, full-auto)"
        )),
    }
}

/// Parses the process's arguments and runs. Never returns an `Err`; failures
/// are printed to stderr and become exit code `1`.
pub fn exec_main() -> ExitCode {
    exec_main_from(std::env::args_os())
}

/// Like [`exec_main`], with explicit arguments (the first is the program name).
pub fn exec_main_from<I, T>(args: I) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let args = match ExecArgs::try_parse_from(args) {
        Ok(a) => a,
        Err(e) => {
            let _ = e.print();
            return ExitCode::from(if e.use_stderr() { 1 } else { 0 });
        }
    };
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(1);
        }
    };
    match rt.block_on(run(args)) {
        Ok(code) => ExitCode::from(code),
        Err(msg) => {
            eprintln!("error: {msg}");
            ExitCode::from(1)
        }
    }
}

fn guess_provider(model: &str) -> String {
    if let Ok(p) = std::env::var("COSMOS_PROVIDER") {
        return p;
    }
    if model.starts_with("claude") {
        "anthropic".into()
    } else if model.contains('/') {
        "openrouter".into()
    } else {
        Config::new().default_provider().to_owned()
    }
}

async fn run(args: ExecArgs) -> Result<u8, String> {
    let prompt = match args.prompt.as_deref() {
        Some(p) if p != "-" => p.to_owned(),
        _ => {
            if std::io::stdin().is_terminal() {
                return Err("no prompt given (pass one, or pipe it on stdin)".into());
            }
            let mut s = String::new();
            std::io::stdin()
                .read_to_string(&mut s)
                .map_err(|e| format!("reading stdin: {e}"))?;
            s
        }
    };
    if prompt.trim().is_empty() {
        return Err("the prompt is empty".into());
    }

    let cwd = match &args.cwd {
        Some(c) => c.clone(),
        None => std::env::current_dir().map_err(|e| e.to_string())?,
    };
    let loaded = CoreConfig::load(&LoadOptions::new(&cwd)).map_err(|e| e.to_string())?;
    let mut config = loaded.config;
    config.cwd = Some(cwd.clone());
    if let Some(m) = &args.model {
        config.model = Some(m.clone());
    }
    if let Some(mode) = args.mode {
        config.permissions.mode = mode;
    }
    if let Some(n) = args.max_turns {
        config.max_steps = n;
    }
    if args.ephemeral {
        config.sessions.enabled = false;
    }
    let model = config
        .model
        .clone()
        .ok_or("no model: pass --model or set `model` in config")?;
    let provider = args
        .provider
        .clone()
        .unwrap_or_else(|| guess_provider(&model));
    let client = Client::from_config(Config::new(), &provider).map_err(|e| e.to_string())?;

    let fs = Arc::new(DiskFs::new(&cwd).map_err(|e| e.to_string())?);
    let tools = CodingTools::new(fs);
    let mut builder = Core::builder(config)
        .completer(Arc::new(client))
        .coding_tools(&tools)
        .warnings(loaded.warnings);
    if args.continue_session {
        builder = builder.resume(ResumeFrom::Latest);
    } else if let Some(id) = args.resume {
        builder = builder.resume(ResumeFrom::Id(id));
    }
    let (handle, mut events) = builder.start().map_err(|e| e.to_string())?;

    let token = CancellationToken::new();
    {
        let token = token.clone();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                token.cancel();
            }
        });
    }
    let mut opts = ExecOptions::new(args.output);
    opts.max_cost = args.max_cost;
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let result = run_exec(
        &handle,
        &mut events,
        &prompt,
        &opts,
        &token,
        &mut stdout.lock(),
        &mut stderr.lock(),
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(result.exit_code())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_documented_flags() {
        let a = ExecArgs::try_parse_from([
            "exec",
            "fix it",
            "--mode",
            "auto-edit",
            "--output",
            "jsonl",
            "--max-turns",
            "3",
            "--max-cost",
            "1.5",
            "--continue",
        ])
        .unwrap();
        assert_eq!(a.prompt.as_deref(), Some("fix it"));
        assert_eq!(a.mode, Some(ApprovalMode::AutoEdit));
        assert_eq!(a.output, OutputFormat::Jsonl);
        assert_eq!(a.max_turns, Some(3));
        assert!(a.continue_session);
    }

    #[test]
    fn ask_is_not_a_mode_here() {
        assert!(ExecArgs::try_parse_from(["exec", "x", "--mode", "ask"]).is_err());
        assert!(ExecArgs::try_parse_from(["exec", "x", "--resume", "a", "--continue"]).is_err());
    }
}
