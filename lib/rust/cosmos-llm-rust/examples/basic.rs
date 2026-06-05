//! Minimal usage example for cosmos-llm.
//!
//! Run with:
//!   OPENAI_API_KEY=sk-... cargo run --example basic
//!
//! Or for Anthropic:
//!   ANTHROPIC_API_KEY=sk-ant-... cargo run --example basic -- anthropic claude-3-5-sonnet-20241022

use cosmos_llm::{Client, CompletionRequest, Message};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let provider = args.get(1).map(String::as_str).unwrap_or("openai");
    let model = args.get(2).map(String::as_str).unwrap_or("gpt-4o");

    let api_key = match provider {
        "anthropic" => std::env::var("ANTHROPIC_API_KEY")
            .or_else(|_| std::env::var("CLLM__ANTHROPIC__API_KEY"))
            .map_err(|_| "Set ANTHROPIC_API_KEY")?,
        _ => std::env::var("OPENAI_API_KEY")
            .or_else(|_| std::env::var("CLLM__OPENAI__API_KEY"))
            .map_err(|_| "Set OPENAI_API_KEY")?,
    };

    println!("Provider: {provider}  Model: {model}");

    let client = Client::new(provider, api_key)?.with_model(model);

    // Simple one-shot completion.
    let answer = client.complete("What is the capital of France?").await?;
    println!("Simple completion: {answer}");

    // Multi-turn chat with a system message.
    let req = CompletionRequest::new(
        model,
        vec![
            Message::system("You are a concise assistant. Answer in one sentence."),
            Message::user("Name three planets in our solar system."),
        ],
    )
    .with_temperature(0.3)
    .with_max_tokens(128);

    let resp = client.chat(req).await?;
    println!(
        "Chat response: {}",
        resp.content().unwrap_or("(no content)")
    );

    if let Some(usage) = resp.usage {
        println!(
            "Tokens — prompt: {}, completion: {}, total: {}",
            usage.prompt_tokens, usage.completion_tokens, usage.total_tokens
        );
    }

    // List available models.
    let models = client.models().await?;
    println!("Available models ({}):", models.len());
    for m in models.iter().take(5) {
        println!("  {m}");
    }
    if models.len() > 5 {
        println!("  … and {} more", models.len() - 5);
    }

    Ok(())
}
