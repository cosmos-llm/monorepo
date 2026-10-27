//! The `cosmos-llm-adapter` command line.
//!
//! `format` turns a signature and inputs into the messages a model would
//! receive; `parse` turns a saved reply into typed outputs. Between them a
//! prompt can be inspected, and a reply re-parsed, without writing code — or
//! piped through any model CLI in between.
//!
//! [`main`] is public so that a binary linking extra adapters, field types,
//! or named signatures gets the same command line with them included:
//!
//! ```ignore
//! use my_adapter_plugins as _;
//!
//! fn main() -> std::process::ExitCode {
//!     cosmos_llm_adapter::cli::main()
//! }
//! ```

use std::error::Error;
use std::io::{Read, Write};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use serde_json::{json, Map, Value};

use cosmos_llm_signature::Signature;

use crate::{adapter, AdapterPlugin};

/// Render signatures into chat messages and parse model replies.
#[derive(Debug, Parser)]
#[command(name = "cosmos-llm-adapter", version, about, long_about = None)]
#[command(after_help = "\
SPEC is a shorthand string ('question -> answer: int'), @file.json, or the name of a
registered signature.

Examples:
  cosmos-llm-adapter format 'question -> answer' '{\"question\": \"What is 2+2?\"}'
  cosmos-llm-adapter format 'q -> a: int' --adapter json_schema --params '{\"q\": \"hi\"}'
  cosmos-llm-adapter parse 'question -> answer: int' reply.txt
  some-model-cli < prompt.json | cosmos-llm-adapter parse 'q -> a' -
  cosmos-llm-adapter plugins")]
pub struct Cli {
    /// The subcommand to run.
    #[command(subcommand)]
    pub command: Command,
}

/// The subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Print the messages an adapter renders for a signature and inputs.
    ///
    /// Prints a JSON array of {role, content} messages. With --params, prints
    /// {"messages": [...], "params": {...}} instead, where params are the
    /// extra request-body fields the adapter wants (e.g. response_format).
    Format {
        /// The signature.
        spec: String,
        /// The inputs as a JSON object; read from stdin when omitted or `-`.
        inputs: Option<String>,
        /// The adapter to use.
        #[arg(short, long, default_value = "chat")]
        adapter: String,
        /// A JSONL file of demonstrations, one object per line holding both
        /// input and output fields.
        #[arg(short, long, value_name = "FILE")]
        demos: Option<String>,
        /// Also print the adapter's request parameters.
        #[arg(long)]
        params: bool,
    },
    /// Parse a model reply into the signature's typed outputs.
    Parse {
        /// The signature.
        spec: String,
        /// A file holding the reply; read from stdin when omitted or `-`.
        completion: Option<String>,
        /// The adapter to use.
        #[arg(short, long, default_value = "chat")]
        adapter: String,
    },
    /// List registered adapters.
    Plugins {
        /// Print the listing as JSON.
        #[arg(long)]
        json: bool,
    },
}

/// Parses the process arguments, runs the command, and reports errors on
/// standard error.
///
/// Exits 0 on success, 1 when the command fails, and 2 on a usage error.
pub fn main() -> ExitCode {
    let cli = Cli::parse();
    let mut stdout = std::io::stdout().lock();
    match run(cli, &mut std::io::stdin().lock(), &mut stdout) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cosmos-llm-adapter: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Runs a parsed command against the given standard input and output.
///
/// # Errors
///
/// Returns the error that stopped the command, already worded for a person.
pub fn run(cli: Cli, stdin: &mut dyn Read, stdout: &mut dyn Write) -> Result<(), Box<dyn Error>> {
    match cli.command {
        Command::Format {
            spec,
            inputs,
            adapter: name,
            demos,
            params,
        } => {
            let sig = Signature::from_spec(&spec)?;
            let chosen = adapter(&name)?;
            let raw = read_arg_or_stdin(inputs.as_deref(), stdin)?;
            let raw: Value = serde_json::from_str(&raw)
                .map_err(|e| format!("inputs are not valid JSON: {e}"))?;
            let inputs = sig.coerce_inputs(&raw)?;
            let demos = match demos {
                Some(path) => read_demos(&path)?,
                None => Vec::new(),
            };
            let messages = serde_json::to_value(chosen.format(&sig, &inputs, &demos)?)?;
            let output = if params {
                json!({ "messages": messages, "params": chosen.request_params(&sig)? })
            } else {
                messages
            };
            writeln!(stdout, "{}", serde_json::to_string_pretty(&output)?)?;
        }
        Command::Parse {
            spec,
            completion,
            adapter: name,
        } => {
            let sig = Signature::from_spec(&spec)?;
            let chosen = adapter(&name)?;
            let text = match completion.as_deref() {
                Some(path) if path != "-" => {
                    std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?
                }
                _ => read_arg_or_stdin(None, stdin)?,
            };
            let outputs = chosen.parse(&sig, &text)?;
            writeln!(stdout, "{}", Value::Object(outputs))?;
        }
        Command::Plugins { json } => write_plugins(json, stdout)?,
    }
    Ok(())
}

fn read_arg_or_stdin(arg: Option<&str>, stdin: &mut dyn Read) -> Result<String, Box<dyn Error>> {
    match arg {
        Some(text) if text != "-" => Ok(text.to_owned()),
        _ => {
            let mut buffer = String::new();
            stdin.read_to_string(&mut buffer)?;
            Ok(buffer)
        }
    }
}

fn read_demos(path: &str) -> Result<Vec<Map<String, Value>>, Box<dyn Error>> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?;
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| match serde_json::from_str::<Value>(line) {
            Ok(Value::Object(object)) => Ok(object),
            Ok(_) => Err(format!("{path}:{}: expected a JSON object", index + 1).into()),
            Err(e) => Err(format!("{path}:{}: {e}", index + 1).into()),
        })
        .collect()
}

fn write_plugins(json: bool, out: &mut dyn Write) -> Result<(), Box<dyn Error>> {
    let rows = cosmos_llm_registry::listing::<AdapterPlugin>();
    if json {
        let listing: Vec<Value> = rows
            .iter()
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
            .collect();
        writeln!(
            out,
            "{}",
            serde_json::to_string_pretty(&json!({ "adapters": listing }))?
        )?;
        return Ok(());
    }
    writeln!(out, "Adapters (* = in use):")?;
    for row in &rows {
        writeln!(out, "  {row}")?;
    }
    for name in cosmos_llm_registry::conflicts::<AdapterPlugin>() {
        writeln!(
            out,
            "\nwarning: several adapters claim '{name}' at the same priority"
        )?;
    }
    Ok(())
}
