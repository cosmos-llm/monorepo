//! Headless agent: `cargo run --example exec --features cli -- 'list the TODOs' --output jsonl`.
//!
//! Needs a provider key in the environment (for example `ANTHROPIC_API_KEY`)
//! and a model (`--model`, or `model` in `~/.config/cosmos/config.trongo`).

fn main() -> std::process::ExitCode {
    cosmos_llm_agent_core::cli::exec_main()
}
