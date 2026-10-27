//! The `cosmos-llm-cache` command line.
//!
//! Computes cache keys and reads, writes, and clears a cache directory.
//! Values are JSON on standard input and output, so the command composes
//! with `jq` and with the other cosmos-llm CLIs.
//!
//! [`main`] is public so that a binary linking extra stores gets the same
//! command line with them available through `--store`:
//!
//! ```ignore
//! use my_cache_plugins as _;
//!
//! fn main() -> std::process::ExitCode {
//!     cosmos_llm_cache::cli::main()
//! }
//! ```

use std::error::Error;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use serde_json::{json, Value};

use crate::{key_for, store, CacheOptions, CachePlugin, CacheStore, DiskStore};

/// Compute cache keys and inspect or clear a cosmos-llm cache.
#[derive(Debug, Parser)]
#[command(name = "cosmos-llm-cache", version, about, long_about = None)]
#[command(after_help = "\
Examples:
  echo '{\"model\": \"m\", \"prompt\": \"hi\"}' | cosmos-llm-cache key
  cosmos-llm-cache put --dir ~/.cache/cosmos-llm \"$KEY\" '{\"completion\": \"hello\"}'
  cosmos-llm-cache get --dir ~/.cache/cosmos-llm \"$KEY\" | jq .completion
  cosmos-llm-cache stats --dir ~/.cache/cosmos-llm
  cosmos-llm-cache clear --dir ~/.cache/cosmos-llm --yes")]
pub struct Cli {
    /// The subcommand to run.
    #[command(subcommand)]
    pub command: Command,
}

/// Which cache to operate on.
#[derive(Debug, Args)]
pub struct Target {
    /// The cache directory.
    #[arg(short, long, value_name = "DIR")]
    pub dir: PathBuf,
    /// The registered store to open the directory with.
    #[arg(short, long, default_value = "disk")]
    pub store: String,
}

impl Target {
    fn open(&self) -> Result<Box<dyn CacheStore>, Box<dyn Error>> {
        Ok(store(&self.store, &CacheOptions::in_directory(&self.dir))?)
    }
}

/// The subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Print the cache key for a request.
    Key {
        /// The request as JSON; read from stdin when omitted or `-`.
        request: Option<String>,
    },
    /// Print the value stored under a key; exits 1 if there is none.
    Get {
        /// Which cache to read.
        #[command(flatten)]
        target: Target,
        /// The key.
        key: String,
    },
    /// Store a value under a key.
    Put {
        /// Which cache to write.
        #[command(flatten)]
        target: Target,
        /// The key.
        key: String,
        /// The value as JSON; read from stdin when omitted or `-`.
        value: Option<String>,
    },
    /// Remove the entry under a key; exits 1 if there was none.
    Remove {
        /// Which cache to modify.
        #[command(flatten)]
        target: Target,
        /// The key.
        key: String,
    },
    /// Delete every entry. Requires --yes, since it deletes files.
    ///
    /// Only files the cache wrote are deleted: entry and temp files in its
    /// shard directories. Anything else in the directory is left alone.
    Clear {
        /// Which cache to clear.
        #[command(flatten)]
        target: Target,
        /// Confirm the deletion.
        #[arg(long)]
        yes: bool,
    },
    /// Count the entries in a cache directory and their size on disk.
    Stats {
        /// The cache directory.
        #[arg(short, long, value_name = "DIR")]
        dir: PathBuf,
        /// Print the counts as JSON.
        #[arg(long)]
        json: bool,
    },
    /// List registered cache stores.
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
            eprintln!("cosmos-llm-cache: {e}");
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
        Command::Key { request } => {
            let request = read_json(request.as_deref(), stdin, "request")?;
            writeln!(stdout, "{}", key_for(&request))?;
        }
        Command::Get { target, key } => match target.open()?.get(&key)? {
            Some(value) => writeln!(stdout, "{value}")?,
            None => return Err(format!("no entry for key {key}").into()),
        },
        Command::Put { target, key, value } => {
            let value = read_json(value.as_deref(), stdin, "value")?;
            target.open()?.put(&key, &value)?;
        }
        Command::Remove { target, key } => {
            if !target.open()?.remove(&key)? {
                return Err(format!("no entry for key {key}").into());
            }
        }
        Command::Clear { target, yes } => {
            if !yes {
                return Err(format!(
                    "refusing to delete the cache in {} without --yes",
                    target.dir.display()
                )
                .into());
            }
            target.open()?.clear()?;
        }
        Command::Stats { dir, json } => {
            if !dir.is_dir() {
                return Err(format!("{} is not a directory", dir.display()).into());
            }
            let stats = DiskStore::new(&dir)?.stats()?;
            if json {
                writeln!(
                    stdout,
                    "{}",
                    json!({ "entries": stats.entries, "bytes": stats.bytes })
                )?;
            } else {
                writeln!(
                    stdout,
                    "{} entries, {} bytes in {}",
                    stats.entries,
                    stats.bytes,
                    dir.display()
                )?;
            }
        }
        Command::Plugins { json } => write_plugins(json, stdout)?,
    }
    Ok(())
}

/// Reads a JSON value from `arg`, or from `stdin` when `arg` is absent or `-`.
fn read_json(arg: Option<&str>, stdin: &mut dyn Read, what: &str) -> Result<Value, Box<dyn Error>> {
    let text = match arg {
        Some(text) if text != "-" => text.to_owned(),
        _ => {
            let mut buffer = String::new();
            stdin.read_to_string(&mut buffer)?;
            buffer
        }
    };
    serde_json::from_str(&text).map_err(|e| format!("{what} is not valid JSON: {e}").into())
}

fn write_plugins(json: bool, out: &mut dyn Write) -> Result<(), Box<dyn Error>> {
    let stores = cosmos_llm_registry::listing::<CachePlugin>();
    if json {
        let rows: Vec<Value> = stores
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
            serde_json::to_string_pretty(&json!({ "stores": rows }))?
        )?;
        return Ok(());
    }
    writeln!(out, "Cache stores (* = in use):")?;
    for row in &stores {
        writeln!(out, "  {row}")?;
    }
    for name in cosmos_llm_registry::conflicts::<CachePlugin>() {
        writeln!(
            out,
            "\nwarning: several cache stores claim '{name}' at the same priority"
        )?;
    }
    Ok(())
}
