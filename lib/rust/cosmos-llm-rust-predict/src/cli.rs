//! The `cosmos-llm-predict` command line.
//!
//! `run` answers one JSON object of inputs, or — with `--jsonl` — one per
//! line of standard input, writing one prediction per line in the same
//! order. Rows that fail print `{}` so the output stays aligned with the
//! input for `cosmos-llm-evaluate score`, and the failure goes to standard
//! error with its line number.
//!
//! [`main`] is public so a binary that links extra backends, adapters, or
//! strategies gets the same command line with them included:
//!
//! ```ignore
//! use my_llm_backend as _;
//!
//! fn main() -> std::process::ExitCode {
//!     cosmos_llm_predict::cli::main()
//! }
//! ```

use std::error::Error;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use futures_util::stream::{self, StreamExt};
use serde_json::{json, Map, Value};

use cosmos_llm_cache::CacheOptions;
use cosmos_llm_example::devset;
use cosmos_llm_signature::Signature;

use crate::{
    BackendOptions, BackendPlugin, CallOptions, Module, Parts, Prediction, StrategyPlugin,
};

/// Answer signatures with a language model.
#[derive(Debug, Parser)]
#[command(name = "cosmos-llm-predict", version, about, long_about = None)]
#[command(after_help = "\
SPEC is a shorthand string ('question -> answer: int'), @file.json, or the name of a
registered signature. API keys are read from the environment as cosmos-llm does.

Examples:
  cosmos-llm-predict run 'question -> answer' '{\"question\": \"Why is the sky blue?\"}' \\
      --provider anthropic --model claude-sonnet-5-5
  cosmos-llm-example inputs dev.jsonl -i question \\
      | cosmos-llm-predict run 'question -> answer' --jsonl --strategy cot \\
          --provider openai --model gpt-5 --cache-dir ~/.cache/cosmos-llm > preds.jsonl
  cosmos-llm-predict plugins")]
pub struct Cli {
    /// The subcommand to run.
    #[command(subcommand)]
    pub command: Command,
}

/// The subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run a signature on inputs and print the prediction.
    Run(Box<RunArgs>),
    /// List registered backends and strategies.
    Plugins {
        /// Print the listing as JSON.
        #[arg(long)]
        json: bool,
    },
}

/// Arguments to `run`.
#[derive(Debug, clap::Args)]
pub struct RunArgs {
    /// The signature.
    pub spec: String,
    /// The inputs as a JSON object; read from stdin when omitted or `-`.
    pub inputs: Option<String>,
    /// Read one JSON object of inputs per line of stdin.
    #[arg(long)]
    pub jsonl: bool,
    /// How many rows to run at once with --jsonl; output order is kept.
    #[arg(long, default_value_t = 1)]
    pub concurrency: usize,
    /// The language model backend.
    #[arg(long, default_value = "cosmos")]
    pub backend: String,
    /// The provider, for backends that front several.
    #[arg(short, long)]
    pub provider: Option<String>,
    /// The model.
    #[arg(short, long)]
    pub model: Option<String>,
    /// An alternative API root, for proxies and compatible servers.
    #[arg(long)]
    pub base_url: Option<String>,
    /// The prompting strategy.
    #[arg(short, long, default_value = "predict")]
    pub strategy: String,
    /// The adapter that renders prompts and parses replies.
    #[arg(short, long)]
    pub adapter: Option<String>,
    /// Cache replies in this directory.
    #[arg(long)]
    pub cache_dir: Option<PathBuf>,
    /// A devset of demonstrations to show the model.
    #[arg(long)]
    pub demos: Option<PathBuf>,
    /// Replace the signature's instructions.
    #[arg(short, long)]
    pub instructions: Option<String>,
    /// Sampling temperature.
    #[arg(short, long)]
    pub temperature: Option<f32>,
    /// Maximum tokens to generate.
    #[arg(long)]
    pub max_tokens: Option<u32>,
    /// Print the raw reply, usage, and cache status along with the values.
    #[arg(long)]
    pub full: bool,
}

/// Parses the process arguments, runs the command, and reports errors on
/// standard error.
///
/// Exits 0 on success, 1 when the command (or any `--jsonl` row) fails, and 2
/// on a usage error.
pub fn main() -> ExitCode {
    let cli = Cli::parse();
    let mut stdout = std::io::stdout().lock();
    match run(cli, &mut std::io::stdin().lock(), &mut stdout) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cosmos-llm-predict: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Runs a parsed command against the given standard input and output.
///
/// Builds its own Tokio runtime, so it must not be called from inside one.
///
/// # Errors
///
/// Returns the error that stopped the command, already worded for a person.
pub fn run(cli: Cli, stdin: &mut dyn Read, stdout: &mut dyn Write) -> Result<(), Box<dyn Error>> {
    match cli.command {
        Command::Run(args) => {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;
            runtime.block_on(run_signature(*args, stdin, stdout))
        }
        Command::Plugins { json } => write_plugins(json, stdout),
    }
}

async fn run_signature(
    args: RunArgs,
    stdin: &mut dyn Read,
    stdout: &mut dyn Write,
) -> Result<(), Box<dyn Error>> {
    let module = build_module(&args)?;
    let options = CallOptions {
        temperature: args.temperature,
        max_tokens: args.max_tokens,
        ..CallOptions::default()
    };

    if !args.jsonl {
        let inputs = match args.inputs.as_deref() {
            Some(text) if text != "-" => text.to_owned(),
            _ => {
                let mut buffer = String::new();
                stdin.read_to_string(&mut buffer)?;
                buffer
            }
        };
        let inputs = parse_object(&inputs).map_err(|e| format!("inputs: {e}"))?;
        let prediction = module.forward(&inputs, &options).await?;
        writeln!(stdout, "{}", render(&prediction, args.full))?;
        return Ok(());
    }

    if args.inputs.as_deref().is_some_and(|text| text != "-") {
        return Err("--jsonl reads inputs from stdin; drop the INPUTS argument".into());
    }
    let rows: Vec<(usize, String)> = BufReader::new(stdin)
        .lines()
        .enumerate()
        .map(|(index, line)| line.map(|text| (index + 1, text)))
        .collect::<Result<_, _>>()?;
    let rows = rows.into_iter().filter(|(_, text)| !text.trim().is_empty());

    let module = &module;
    let options = &options;
    let mut results = stream::iter(rows)
        .map(|(line, text)| async move {
            let outcome = match parse_object(&text) {
                Ok(inputs) => module
                    .forward(&inputs, options)
                    .await
                    .map_err(|e| e.to_string()),
                Err(e) => Err(e),
            };
            (line, outcome)
        })
        .buffered(args.concurrency.max(1));

    let mut failures = 0usize;
    while let Some((line, outcome)) = results.next().await {
        match outcome {
            Ok(prediction) => writeln!(stdout, "{}", render(&prediction, args.full))?,
            Err(message) => {
                failures += 1;
                eprintln!("cosmos-llm-predict: line {line}: {message}");
                writeln!(stdout, "{{}}")?;
            }
        }
    }
    if failures > 0 {
        return Err(format!("{failures} row(s) failed; see the messages above").into());
    }
    Ok(())
}

fn build_module(args: &RunArgs) -> Result<Box<dyn Module>, Box<dyn Error>> {
    let mut signature = Signature::from_spec(&args.spec)?;
    if let Some(text) = &args.instructions {
        signature = signature.with_instructions(text.clone());
    }

    let model = crate::backend(
        &args.backend,
        &BackendOptions {
            provider: args.provider.clone(),
            model: args.model.clone(),
            api_key: None,
            base_url: args.base_url.clone(),
        },
    )?;

    let mut parts = Parts::new(signature, Arc::from(model));
    if let Some(name) = &args.adapter {
        parts.adapter = Some(Arc::from(cosmos_llm_adapter::adapter(name)?));
    }
    if let Some(dir) = &args.cache_dir {
        parts.cache = Some(Arc::from(cosmos_llm_cache::store(
            "tiered",
            &CacheOptions::in_directory(dir),
        )?));
    }
    if let Some(path) = &args.demos {
        parts.demos = devset::load(path, &[])?;
    }
    Ok(crate::strategy(&args.strategy, parts)?)
}

fn parse_object(text: &str) -> Result<Map<String, Value>, String> {
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(object)) => Ok(object),
        Ok(other) => Err(format!("expected a JSON object, got {other}")),
        Err(e) => Err(format!("not valid JSON: {e}")),
    }
}

fn render(prediction: &Prediction, full: bool) -> Value {
    if full {
        serde_json::to_value(prediction).unwrap_or(Value::Null)
    } else {
        Value::Object(prediction.values.clone())
    }
}

fn write_plugins(json: bool, out: &mut dyn Write) -> Result<(), Box<dyn Error>> {
    let backends = cosmos_llm_registry::listing::<BackendPlugin>();
    let strategies = cosmos_llm_registry::listing::<StrategyPlugin>();
    if json {
        let rows = |rows: &[cosmos_llm_registry::Listing]| {
            rows.iter()
                .map(|row| {
                    json!({
                        "name": row.meta.name,
                        "aliases": row.meta.aliases,
                        "priority": row.meta.priority,
                        "summary": row.meta.summary,
                        "origin": row.meta.origin,
                        "active": row.active,
                    })
                })
                .collect::<Vec<_>>()
        };
        let listing = json!({ "backends": rows(&backends), "strategies": rows(&strategies) });
        writeln!(out, "{}", serde_json::to_string_pretty(&listing)?)?;
        return Ok(());
    }
    for (title, rows) in [("Backends", &backends), ("Strategies", &strategies)] {
        writeln!(out, "{title} (* = in use):")?;
        for row in rows {
            writeln!(out, "  {row}")?;
        }
        writeln!(out)?;
    }
    writeln!(out, "Adapters and cache stores: see `cosmos-llm-adapter plugins` and `cosmos-llm-cache plugins`.")?;
    Ok(())
}
