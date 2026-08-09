require "spec"
require "../src/cosmos-llm"

# Removes every `CLLM__*` variable plus the provider-native keys, so specs
# never pick up credentials from the developer's shell.
def with_clean_env(&)
  saved = {} of String => String

  keys = ENV.keys.select do |key|
    key.starts_with?("CLLM__") || key.in?("OPENAI_API_KEY", "ANTHROPIC_API_KEY")
  end

  keys.each do |key|
    saved[key] = ENV[key]
    ENV.delete(key)
  end

  begin
    yield
  ensure
    saved.each { |key, value| ENV[key] = value }
  end
end
