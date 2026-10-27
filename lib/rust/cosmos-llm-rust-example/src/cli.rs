//! The `cosmos-llm-example` command line.
//!
//! Reads a devset from a file or standard input (`-`) and writes JSON Lines
//! or readable text to standard output. [`main`] is public so a binary that
//! links extra devset formats gets the same command line with them included:
//!
//! ```ignore
//! use my_devset_formats as _;
//!
//! fn main() -> std::process::ExitCode {
//!     cosmos_llm_example::cli::main()
//! }
//! ```

use std::error::Error;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use serde_json::{json, Map, Value};

use crate::devset::{self, DevsetFormat, FormatPlugin};
use crate::Example;

/// Inspect, split, and project devsets of labeled examples.
#[derive(Debug, Parser)]
#[command(name = "cosmos-llm-example", version, about, long_about = None)]
#[command(after_help = "\
Examples:
  cosmos-llm-example inspect dev.jsonl --inputs question
  cosmos-llm-example inputs dev.jsonl --inputs question,context > inputs.jsonl
  cosmos-llm-example split all.jsonl --train 0.8 --seed 7 --train-out train.jsonl --dev-out dev.jsonl
  cat rows.json | cosmos-llm-example convert - --format json --to jsonl")]
pub struct Cli {
    /// The subcommand to run.
    #[command(subcommand)]
    pub command: Command,
}

/// Where a devset comes from.
#[derive(Debug, Args)]
pub struct Source {
    /// The devset file, or `-` for standard input.
    pub file: String,
    /// The format; chosen from the file extension when omitted, and `jsonl`
    /// for standard input.
    #[arg(short, long)]
    pub format: Option<String>,
    /// Comma-separated keys to treat as inputs.
    #[arg(short, long, value_delimiter = ',')]
    pub inputs: Vec<String>,
}

/// The subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Summarize a devset: row count, keys, and rows missing inputs.
    Inspect {
        /// The devset.
        #[command(flatten)]
        source: Source,
        /// Print the summary as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Print each example's input values as JSON Lines.
    Inputs {
        /// The devset.
        #[command(flatten)]
        source: Source,
    },
    /// Print each example's label values as JSON Lines.
    Labels {
        /// The devset.
        #[command(flatten)]
        source: Source,
    },
    /// Shuffle with a seed and split into training and held-out files.
    Split {
        /// The devset.
        #[command(flatten)]
        source: Source,
        /// The fraction of examples that go to training.
        #[arg(long, default_value_t = 0.8)]
        train: f64,
        /// The shuffle seed; the same seed always gives the same split.
        #[arg(long, default_value_t = 0)]
        seed: u64,
        /// Where to write the training examples.
        #[arg(long)]
        train_out: PathBuf,
        /// Where to write the held-out examples.
        #[arg(long)]
        dev_out: PathBuf,
        /// Overwrite output files that already exist.
        #[arg(long)]
        force: bool,
    },
    /// Rewrite a devset in another format.
    Convert {
        /// The devset.
        #[command(flatten)]
        source: Source,
        /// The format to write.
        #[arg(long, default_value = "jsonl")]
        to: String,
    },
    /// List registered devset formats.
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
            eprintln!("cosmos-llm-example: {e}");
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
        Command::Inspect { source, json } => {
            let examples = load(&source, stdin)?;
            write_summary(&examples, json, stdout)?;
        }
        Command::Inputs { source } => {
            for example in load(&source, stdin)? {
                writeln!(stdout, "{}", Value::Object(example.inputs()))?;
            }
        }
        Command::Labels { source } => {
            if source.inputs.is_empty() {
                return Err(
                    "labels needs --inputs, since without input keys every value is an input"
                        .into(),
                );
            }
            for example in load(&source, stdin)? {
                writeln!(stdout, "{}", Value::Object(example.labels()))?;
            }
        }
        Command::Split {
            source,
            train,
            seed,
            train_out,
            dev_out,
            force,
        } => {
            for path in [&train_out, &dev_out] {
                if path.exists() && !force {
                    return Err(format!(
                        "{} already exists; pass --force to overwrite it",
                        path.display()
                    )
                    .into());
                }
            }
            let (train_set, dev_set) = devset::split(load(&source, stdin)?, train, seed)?;
            write_file(&train_out, &train_set)?;
            write_file(&dev_out, &dev_set)?;
            writeln!(
                stdout,
                "{} training examples -> {}\n{} held-out examples -> {}",
                train_set.len(),
                train_out.display(),
                dev_set.len(),
                dev_out.display()
            )?;
        }
        Command::Convert { source, to } => {
            let examples = load(&source, stdin)?;
            write!(
                stdout,
                "{}",
                devset::write(&examples, devset::format(&to)?)?
            )?;
        }
        Command::Plugins { json } => {
            let rows = cosmos_llm_registry::listing::<FormatPlugin>();
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
                writeln!(stdout, "{}", serde_json::to_string_pretty(&listing)?)?;
            } else {
                writeln!(stdout, "Devset formats (* = in use):")?;
                for row in rows {
                    writeln!(stdout, "  {row}")?;
                }
            }
        }
    }
    Ok(())
}

fn load(source: &Source, stdin: &mut dyn Read) -> Result<Vec<Example>, Box<dyn Error>> {
    let keys: Vec<&str> = source.inputs.iter().map(String::as_str).collect();
    let (text, format): (String, &dyn DevsetFormat) = if source.file == "-" {
        let mut text = String::new();
        stdin.read_to_string(&mut text)?;
        (
            text,
            devset::format(source.format.as_deref().unwrap_or("jsonl"))?,
        )
    } else {
        let path = Path::new(&source.file);
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let format = match &source.format {
            Some(name) => devset::format(name)?,
            None => devset::format_for_path(path)?,
        };
        (text, format)
    };
    Ok(devset::read(&text, format, &keys)?)
}

fn write_file(path: &Path, examples: &[Example]) -> Result<(), Box<dyn Error>> {
    let text = devset::write(examples, devset::format_for_path(path)?)?;
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()).into())
}

fn write_summary(
    examples: &[Example],
    json: bool,
    out: &mut dyn Write,
) -> Result<(), Box<dyn Error>> {
    // Keys in first-seen order, with how many rows carry each.
    let mut keys: Map<String, Value> = Map::new();
    for example in examples {
        for key in example.values().keys() {
            let count = keys.get(key).and_then(Value::as_u64).unwrap_or(0);
            keys.insert(key.clone(), json!(count + 1));
        }
    }
    let incomplete: Vec<Value> = examples
        .iter()
        .enumerate()
        .filter(|(_, example)| !example.missing_inputs().is_empty())
        .map(|(index, example)| json!({ "row": index + 1, "missing": example.missing_inputs() }))
        .collect();

    if json {
        let summary = json!({ "rows": examples.len(), "keys": keys, "missing_inputs": incomplete });
        writeln!(out, "{}", serde_json::to_string_pretty(&summary)?)?;
        return Ok(());
    }
    writeln!(out, "{} examples", examples.len())?;
    let inputs: &[String] = examples
        .first()
        .map(Example::input_keys)
        .unwrap_or_default();
    for (key, count) in &keys {
        let role = if inputs.contains(key) {
            "input"
        } else if inputs.is_empty() {
            ""
        } else {
            "label"
        };
        writeln!(
            out,
            "  {key:<20} {role:<6} in {count} of {}",
            examples.len()
        )?;
    }
    if !incomplete.is_empty() {
        writeln!(out, "\n{} rows are missing inputs:", incomplete.len())?;
        for row in incomplete.iter().take(10) {
            writeln!(out, "  row {}: {}", row["row"], row["missing"])?;
        }
        if incomplete.len() > 10 {
            writeln!(out, "  ... and {} more", incomplete.len() - 10)?;
        }
    }
    Ok(())
}
