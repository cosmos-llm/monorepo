//! Evaluates `models.trongo` into `$OUT_DIR/models.json`.
//!
//! The table is authored in TrongoScript so model families can share defaults
//! through functions, but the library only ever reads the JSON, so loading it
//! needs no evaluator.

use std::{env, fs, path::PathBuf};

use trongoscript::{eval_source, Context, EvalConfig};

fn main() {
    println!("cargo:rerun-if-changed=models.trongo");
    println!("cargo:rerun-if-changed=build.rs");

    let source = fs::read_to_string("models.trongo").expect("read models.trongo");
    let mut ctx = Context::new(EvalConfig::default());
    let value = eval_source(&source, &mut ctx)
        .unwrap_or_else(|e| panic!("models.trongo failed to evaluate: {e}"));

    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR")).join("models.json");
    fs::write(out, serde_json::to_string(&value).expect("serialize table")).expect("write table");
}
