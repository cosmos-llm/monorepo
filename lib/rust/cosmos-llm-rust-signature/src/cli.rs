//! The `cosmos-llm-signature` command line.
//!
//! Every subcommand reads JSON from an argument or from standard input and
//! writes JSON (or, for `show` and `plugins`, readable text) to standard
//! output, so it composes with `jq` and with the other cosmos-llm CLIs.
//!
//! [`main`] is public so that a binary linking extra plugins — custom field
//! types, named signatures — gets the same command line with them included:
//!
//! ```ignore
//! use my_signature_plugins as _;
//!
//! fn main() -> std::process::ExitCode {
//!     cosmos_llm_signature::cli::main()
//! }
//! ```

use std::error::Error;
use std::io::{Read, Write};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use serde_json::{json, Value};

use crate::codec::CodecPlugin;
use crate::{Field, Signature, SignaturePlugin};

/// Inspect LLM call signatures and coerce values through them.
#[derive(Debug, Parser)]
#[command(name = "cosmos-llm-signature", version, about, long_about = None)]
#[command(after_help = "\
SPEC is a shorthand string ('question -> answer: int'), @file.json, or the name of a
registered signature.

Examples:
  cosmos-llm-signature show 'document -> summary, tags: array[string]'
  cosmos-llm-signature schema 'q -> a: int' | jq .properties
  echo '{\"a\": \"42\"}' | cosmos-llm-signature coerce 'q -> a: int'
  cosmos-llm-signature plugins")]
pub struct Cli {
    /// The subcommand to run.
    #[command(subcommand)]
    pub command: Command,
}

/// The subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Describe a signature's fields and instructions.
    Show {
        /// The signature.
        spec: String,
        /// Replace the instructions.
        #[arg(short, long)]
        instructions: Option<String>,
        /// Print the signature as JSON, loadable later with @file.json.
        #[arg(long)]
        json: bool,
    },
    /// Print the JSON schema of a signature's outputs.
    Schema {
        /// The signature.
        spec: String,
    },
    /// Coerce a JSON object of values to the signature's declared types.
    Coerce {
        /// The signature.
        spec: String,
        /// The values as a JSON object; read from stdin when omitted or `-`.
        values: Option<String>,
        /// Coerce against the input fields instead of the outputs.
        #[arg(long)]
        inputs: bool,
    },
    /// List registered field types and named signatures.
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
            eprintln!("cosmos-llm-signature: {e}");
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
        Command::Show {
            spec,
            instructions,
            json,
        } => {
            let mut sig = Signature::from_spec(&spec)?;
            if let Some(text) = instructions {
                sig = sig.with_instructions(text);
            }
            if json {
                writeln!(stdout, "{}", serde_json::to_string_pretty(&sig)?)?;
            } else {
                write_description(&sig, stdout)?;
            }
        }
        Command::Schema { spec } => {
            let sig = Signature::from_spec(&spec)?;
            writeln!(
                stdout,
                "{}",
                serde_json::to_string_pretty(&sig.output_json_schema()?)?
            )?;
        }
        Command::Coerce {
            spec,
            values,
            inputs,
        } => {
            let sig = Signature::from_spec(&spec)?;
            let values = read_json(values.as_deref(), stdin)?;
            let coerced = if inputs {
                sig.coerce_inputs(&values)?
            } else {
                sig.coerce_outputs(&values)?
            };
            writeln!(stdout, "{}", Value::Object(coerced))?;
        }
        Command::Plugins { json } => write_plugins(json, stdout)?,
    }
    Ok(())
}

/// Reads a JSON value from `arg`, or from `stdin` when `arg` is absent or `-`.
fn read_json(arg: Option<&str>, stdin: &mut dyn Read) -> Result<Value, Box<dyn Error>> {
    let text = match arg {
        Some(text) if text != "-" => text.to_owned(),
        _ => {
            let mut buffer = String::new();
            stdin.read_to_string(&mut buffer)?;
            buffer
        }
    };
    serde_json::from_str(&text).map_err(|e| format!("values are not valid JSON: {e}").into())
}

fn write_description(sig: &Signature, out: &mut dyn Write) -> Result<(), Box<dyn Error>> {
    writeln!(out, "{sig}\n")?;
    writeln!(
        out,
        "Instructions{}:",
        if sig.has_instructions() {
            ""
        } else {
            " (generated)"
        }
    )?;
    writeln!(out, "  {}\n", sig.instructions())?;
    let width = sig.fields().iter().map(|f| f.name.len()).max().unwrap_or(0);
    for (title, fields) in [
        ("Inputs", sig.input_fields().collect::<Vec<_>>()),
        ("Outputs", sig.output_fields().collect::<Vec<_>>()),
    ] {
        if fields.is_empty() {
            continue;
        }
        writeln!(out, "{title}:")?;
        for field in fields {
            writeln!(out, "  {}", describe(field, width)?)?;
        }
    }
    Ok(())
}

fn describe(field: &Field, width: usize) -> Result<String, Box<dyn Error>> {
    let mut line = format!("{:<width$}  {}", field.name, field.type_hint()?);
    if !field.required {
        line.push_str(" (optional)");
    }
    if !field.desc.is_empty() {
        line.push_str(" — ");
        line.push_str(&field.desc);
    }
    Ok(line)
}

fn write_plugins(json: bool, out: &mut dyn Write) -> Result<(), Box<dyn Error>> {
    let types = cosmos_llm_registry::listing::<CodecPlugin>();
    let signatures = cosmos_llm_registry::listing::<SignaturePlugin>();
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
        let listing = json!({ "types": rows(&types), "signatures": rows(&signatures) });
        writeln!(out, "{}", serde_json::to_string_pretty(&listing)?)?;
        return Ok(());
    }
    writeln!(out, "Field types (* = in use):")?;
    for row in &types {
        writeln!(out, "  {row}")?;
    }
    writeln!(out, "\nNamed signatures:")?;
    if signatures.is_empty() {
        writeln!(out, "  (none registered)")?;
    }
    for row in &signatures {
        writeln!(out, "  {row}")?;
    }
    for name in cosmos_llm_registry::conflicts::<CodecPlugin>() {
        writeln!(
            out,
            "\nwarning: several field types claim '{name}' at the same priority"
        )?;
    }
    Ok(())
}
