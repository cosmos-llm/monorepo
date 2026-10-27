//! The `cosmos-llm-evaluate` command line.
//!
//! `score` is the offline half of an evaluation: run a program however you
//! like, write its predictions one JSON object per line in devset order, and
//! score them here. `evidence` checks quotes in a record against a source
//! document.
//!
//! [`main`] is public so that a binary linking extra metrics gets the same
//! command line with them included:
//!
//! ```ignore
//! use my_metrics as _;
//!
//! fn main() -> std::process::ExitCode {
//!     cosmos_llm_evaluate::cli::main()
//! }
//! ```

use std::error::Error;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use serde_json::{json, Value};

use cosmos_llm_example::devset;

use crate::{
    evidence, metric, score_predictions, MetricOptions, MetricPlugin, Report, Row, Values,
};

/// Score predictions against a labeled devset, and verify quoted evidence.
#[derive(Debug, Parser)]
#[command(name = "cosmos-llm-evaluate", version, about, long_about = None)]
#[command(after_help = "\
Examples:
  cosmos-llm-evaluate score dev.jsonl preds.jsonl --metric f1
  cosmos-llm-evaluate score dev.jsonl preds.jsonl --metric exact_match --param normalize=false
  cosmos-llm-evaluate score dev.jsonl preds.jsonl --metric grounded --field findings \\
      --param source=document --fail-under 0.9
  cosmos-llm-evaluate evidence --source report.txt finding.json
  cosmos-llm-evaluate plugins")]
pub struct Cli {
    /// The subcommand to run.
    #[command(subcommand)]
    pub command: Command,
}

/// The subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Score a predictions file against a devset, row by row.
    Score {
        /// The labeled devset; format chosen by extension.
        devset: PathBuf,
        /// The predictions, one object per devset row, in the same order;
        /// format chosen by extension.
        predictions: PathBuf,
        /// The metric to score with.
        #[arg(short, long)]
        metric: String,
        /// The field the metric compares.
        #[arg(long, default_value = "answer")]
        field: String,
        /// A metric parameter as key=value; the value is read as JSON when it
        /// parses, and as a string otherwise. Repeatable.
        #[arg(short, long = "param", value_name = "KEY=VALUE")]
        params: Vec<String>,
        /// The score a row needs to count as passed.
        #[arg(long, default_value_t = 1.0)]
        threshold: f64,
        /// Print the full report as JSON.
        #[arg(long)]
        json: bool,
        /// How many of the worst rows to show.
        #[arg(long, default_value_t = 5)]
        show_failures: usize,
        /// Exit with an error when the overall score is below this, for CI.
        #[arg(long)]
        fail_under: Option<f64>,
    },
    /// Check that every quote in a JSON record appears in a source document.
    ///
    /// Exits with an error when any quote is not found.
    Evidence {
        /// The source document, as text.
        #[arg(short, long)]
        source: PathBuf,
        /// The field name holding quotes.
        #[arg(short, long, default_value = "evidence")]
        key: String,
        /// The record as JSON, or a path to one; read from stdin when omitted
        /// or `-`.
        record: Option<String>,
        /// Print the result as JSON.
        #[arg(long)]
        json: bool,
    },
    /// List registered metrics.
    Plugins {
        /// Print the listing as JSON.
        #[arg(long)]
        json: bool,
    },
}

/// Parses the process arguments, runs the command, and reports errors on
/// standard error.
///
/// Exits 0 on success, 1 when the command fails — including a score under
/// `--fail-under` or an unverified quote — and 2 on a usage error.
pub fn main() -> ExitCode {
    let cli = Cli::parse();
    let mut stdout = std::io::stdout().lock();
    match run(cli, &mut std::io::stdin().lock(), &mut stdout) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("cosmos-llm-evaluate: {e}");
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
        Command::Score {
            devset: devset_path,
            predictions,
            metric: name,
            field,
            params,
            threshold,
            json,
            show_failures,
            fail_under,
        } => {
            let mut options = MetricOptions::new(field.clone());
            for param in &params {
                let (key, value) = parse_param(param)?;
                options = options.param(key, value);
            }
            let metric = metric(&name, &options)?;
            let examples = devset::load(&devset_path, &[])?;
            let predictions: Vec<Values> = devset::load(&predictions, &[])?
                .into_iter()
                .map(|example| example.into_values())
                .collect();
            let report = score_predictions(metric.as_ref(), &examples, &predictions)?;

            if json {
                writeln!(stdout, "{}", serde_json::to_string_pretty(&report)?)?;
            } else {
                write_report(&report, &name, &field, threshold, show_failures, stdout)?;
            }
            if let Some(floor) = fail_under {
                if report.score() < floor {
                    return Err(format!(
                        "score {:.3} is below --fail-under {floor}",
                        report.score()
                    )
                    .into());
                }
            }
        }
        Command::Evidence {
            source,
            key,
            record,
            json,
        } => {
            let text = read_file(&source)?;
            let record = read_record(record.as_deref(), stdin)?;
            let quotes = evidence::collect(&record, &key).len();
            let unverified = evidence::unverified(&record, &text, &key);
            if json {
                let result = json!({ "quotes": quotes, "unverified": unverified });
                writeln!(stdout, "{}", serde_json::to_string_pretty(&result)?)?;
            } else if unverified.is_empty() {
                writeln!(stdout, "all {quotes} quotes found in {}", source.display())?;
            } else {
                for message in &unverified {
                    writeln!(stdout, "{message}")?;
                }
            }
            if !unverified.is_empty() {
                return Err(format!(
                    "{} of {quotes} quotes not found in {}",
                    unverified.len(),
                    source.display()
                )
                .into());
            }
        }
        Command::Plugins { json } => {
            let rows = cosmos_llm_registry::listing::<MetricPlugin>();
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
                writeln!(stdout, "Metrics (* = in use):")?;
                for row in rows {
                    writeln!(stdout, "  {row}")?;
                }
                for name in cosmos_llm_registry::conflicts::<MetricPlugin>() {
                    writeln!(
                        stdout,
                        "\nwarning: several metrics claim '{name}' at the same priority"
                    )?;
                }
            }
        }
    }
    Ok(())
}

/// Splits `key=value`, reading the value as JSON when it parses.
fn parse_param(param: &str) -> Result<(String, Value), Box<dyn Error>> {
    let (key, raw) = param
        .split_once('=')
        .ok_or_else(|| format!("--param expects KEY=VALUE, got {param:?}"))?;
    let value = serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_owned()));
    Ok((key.trim().to_owned(), value))
}

fn read_file(path: &Path) -> Result<String, Box<dyn Error>> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()).into())
}

/// Reads a record from inline JSON, a file path, or stdin.
fn read_record(arg: Option<&str>, stdin: &mut dyn Read) -> Result<Value, Box<dyn Error>> {
    let text = match arg {
        None | Some("-") => {
            let mut buffer = String::new();
            stdin.read_to_string(&mut buffer)?;
            buffer
        }
        Some(arg) if Path::new(arg).is_file() => read_file(Path::new(arg))?,
        Some(inline) => inline.to_owned(),
    };
    serde_json::from_str(&text).map_err(|e| format!("record is not valid JSON: {e}").into())
}

fn write_report(
    report: &Report,
    metric: &str,
    field: &str,
    threshold: f64,
    show: usize,
    out: &mut dyn Write,
) -> Result<(), Box<dyn Error>> {
    writeln!(out, "{metric} on {field}: {}", report.summary_at(threshold))?;
    let failures = report.failures(threshold);
    if failures.is_empty() || show == 0 {
        return Ok(());
    }
    writeln!(
        out,
        "\nWorst {} of {} below {threshold}:",
        show.min(failures.len()),
        failures.len()
    )?;
    for row in failures.into_iter().take(show) {
        writeln!(out, "  {}", describe(row, field))?;
    }
    Ok(())
}

fn describe(row: &Row, field: &str) -> String {
    let expected = excerpt(row.example.get(field));
    let mut line = format!(
        "row {:<4} score {:.2}  expected {expected}",
        row.index + 1,
        row.score
    );
    if let Some(prediction) = &row.prediction {
        line.push_str(&format!("  got {}", excerpt(prediction.get(field))));
    }
    if let Some(error) = &row.error {
        line.push_str(&format!("  error: {error}"));
    }
    line
}

fn excerpt(value: Option<&Value>) -> String {
    const LIMIT: usize = 60;
    let text = match value {
        None => return "(missing)".to_owned(),
        Some(value) => value.to_string(),
    };
    if text.chars().count() <= LIMIT {
        return text;
    }
    let cut: String = text.chars().take(LIMIT).collect();
    format!("{cut}…")
}
