# Basic usage of the cosmos-llm shard.
#
# Run with an API key in the environment:
#
#     OPENAI_API_KEY=sk-... crystal run examples/basic.cr

require "../src/cosmos-llm"

client = CosmosLLM::Client.new("openai").with_model("gpt-4o")

# One-shot completion.
puts client.complete("What is the capital of France?")

# Full chat with a system message.
response = client.chat(
  [
    CosmosLLM::Message.system("You are a concise assistant."),
    CosmosLLM::Message.user("Name three planets."),
  ],
  temperature: 0.5,
  max_tokens: 256,
)

puts response.text

if usage = response.usage
  puts "tokens: #{usage.total_tokens}"
end

# Switching providers at runtime.
claude = CosmosLLM::Client.new("anthropic").with_model("claude-sonnet-4-6")
puts claude.complete("Summarize the Apollo program in one sentence.")
