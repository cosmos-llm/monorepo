//! Streaming usage example for cosmos-llm.
//!
//! Run with:
//!   OPENAI_API_KEY=sk-... cargo run --example streaming
//!
//! Or for Anthropic:
//!   ANTHROPIC_API_KEY=sk-ant-... cargo run --example streaming -- anthropic claude-3-5-sonnet-20241022

use std::io::Write;

use cosmos_llm::{Client, CompletionRequest, Message, StreamAccumulator};
use futures_util::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let provider = args.get(1).map(String::as_str).unwrap_or("openai");
    let model = args.get(2).map(String::as_str).unwrap_or("gpt-4o");

    let api_key = match provider {
        "anthropic" => std::env::var("ANTHROPIC_API_KEY")
            .or_else(|_| std::env::var("CLLM__ANTHROPIC__API_KEY"))
            .map_err(|_| "Set ANTHROPIC_API_KEY")?,
        "openrouter" => std::env::var("OPENROUTER_API_KEY")
            .or_else(|_| std::env::var("CLLM__OPENROUTER__API_KEY"))
            .map_err(|_| "Set OPENROUTER_API_KEY")?,
        _ => std::env::var("OPENAI_API_KEY")
            .or_else(|_| std::env::var("CLLM__OPENAI__API_KEY"))
            .map_err(|_| "Set OPENAI_API_KEY")?,
    };

    let client = Client::new(provider, api_key)?.with_model(model);
    println!(
        "Provider: {provider}  Model: {model}  Streaming: {}",
        client.can_stream()
    );

    // ── Simplest form: print tokens as they arrive ───────────────────────────
    println!("\n--- client.stream() ---");
    let mut stream = client
        .stream("Write a haiku about the borrow checker.")
        .await?;
    while let Some(chunk) = stream.next().await {
        print!("{}", chunk?.delta);
        std::io::stdout().flush()?;
    }
    println!();

    // ── Driving the stream by hand, accumulating as you go ───────────────────
    //
    // The accumulator turns the chunk sequence back into the same
    // CompletionResponse a non-streaming call would have returned, so usage
    // and tool calls are available once the stream ends.
    println!("\n--- manual accumulation ---");
    let req = CompletionRequest::new(
        model,
        vec![
            Message::system("You are a concise assistant."),
            Message::user("Name three Rust web frameworks."),
        ],
    )
    .with_max_tokens(200);

    let mut stream = client.stream_completion(req).await?;
    let mut acc = StreamAccumulator::new();

    while let Some(item) = stream.next().await {
        let chunk = item?;
        print!("{}", chunk.delta);
        std::io::stdout().flush()?;
        acc.push(&chunk);
    }
    println!();

    let response = acc.into_response();
    println!("Finish reason: {:?}", response.choices[0].finish_reason);
    if let Some(usage) = response.usage {
        println!(
            "Tokens — prompt: {}, completion: {}, total: {}",
            usage.prompt_tokens, usage.completion_tokens, usage.total_tokens
        );
    }

    // ── One call that both streams and returns the final response ────────────
    println!("\n--- stream_to_completion() ---");
    let req = CompletionRequest::new(model, vec![Message::user("Count from 1 to 5.")])
        .with_max_tokens(64);

    let response = client
        .stream_to_completion(req, |chunk| {
            print!("{}", chunk.delta);
            let _ = std::io::stdout().flush();
        })
        .await?;

    println!("\nCollected {} characters.", response.text().len());

    Ok(())
}
