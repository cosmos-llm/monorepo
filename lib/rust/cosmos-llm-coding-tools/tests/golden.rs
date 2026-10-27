//! Golden files: each IR has a JSON and a TrongoScript fixture, and both must
//! deserialize to the same value and round-trip.
//!
//! Regenerate the `.trongo` files from the JSON with `UPDATE_GOLDEN=1`.

use std::path::PathBuf;

use cosmos_llm_coding_tools::{FileDiff, RevertReport, ShellCommandIr, ShellRun, TurnChanges};
use serde::{de::DeserializeOwned, Serialize};

fn golden(name: &str, ext: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.{ext}"))
}

fn check<T>(name: &str)
where
    T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let json_src = std::fs::read_to_string(golden(name, "json")).unwrap();
    let from_json: T = serde_json::from_str(&json_src).unwrap();

    // JSON round trip: the fixture is the contract.
    let again: serde_json::Value = serde_json::to_value(&from_json).unwrap();
    let want: serde_json::Value = serde_json::from_str(&json_src).unwrap();
    assert_eq!(again, want, "{name}: JSON round trip changed the value");

    #[cfg(feature = "trongo")]
    {
        use cosmos_llm_coding_tools::trongo::{from_trongo, to_trongo};
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            std::fs::write(golden(name, "trongo"), to_trongo(&from_json).unwrap()).unwrap();
        }
        let trongo_src = std::fs::read_to_string(golden(name, "trongo")).unwrap();
        let from_trongo_v: T = from_trongo(&trongo_src).unwrap();
        assert_eq!(from_trongo_v, from_json, "{name}: trongo differs from json");
        // Dump -> load is the identity.
        let dumped = to_trongo(&from_json).unwrap();
        assert_eq!(from_trongo::<T>(&dumped).unwrap(), from_json);
        assert_eq!(dumped.trim(), trongo_src.trim(), "{name}: golden is stale");
    }
}

#[test]
fn file_diff() {
    check::<FileDiff>("file_diff");
}

#[test]
fn turn_changes() {
    check::<TurnChanges>("turn_changes");
}

#[test]
fn revert_report() {
    check::<RevertReport>("revert_report");
}

#[test]
fn shell_command_ir() {
    check::<ShellCommandIr>("shell_command_ir");
}

#[test]
fn shell_command_ir_opaque() {
    check::<ShellCommandIr>("shell_command_ir_opaque");
}

#[test]
fn shell_run() {
    check::<ShellRun>("shell_run");
}

/// The golden tree is what the parser produces for its source line, so the
/// fixture cannot drift from the parser unnoticed.
#[test]
fn shell_command_ir_matches_parser() {
    let json_src = std::fs::read_to_string(golden("shell_command_ir", "json")).unwrap();
    let want: ShellCommandIr = serde_json::from_str(&json_src).unwrap();
    let got = cosmos_llm_coding_tools::parse_shell(
        "FOO=1 cargo test && rm -rf ~ || echo \"failed: $?\" 2>&1 > /tmp/log; \
         (cd sub; ls *.rs) | wc -l &\ncat <<EOF | sh\n$(id)\nEOF\n",
    )
    .unwrap();
    assert_eq!(got, want);
}
