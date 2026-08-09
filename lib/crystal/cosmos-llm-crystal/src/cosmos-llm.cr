# A unified Crystal client for multiple LLM providers.
#
# `cosmos-llm` mirrors the design of the Ruby, Rust, and JavaScript siblings
# in the Cosmos-LLM monorepo: one API surface, pluggable provider backends.
#
# ## Quick start
#
# ```
# require "cosmos-llm"
#
# client = CosmosLLM::Client.new("openai", ENV["OPENAI_API_KEY"])
#   .with_model("gpt-4o")
#
# puts client.complete("What is 2 + 2?")
# ```
#
# ## Environment variables
#
# | Variable                                          | Provider  |
# | ------------------------------------------------- | --------- |
# | `OPENAI_API_KEY` or `CLLM__OPENAI__API_KEY`       | OpenAI    |
# | `ANTHROPIC_API_KEY` or `CLLM__ANTHROPIC__API_KEY` | Anthropic |
#
# ## Supported providers
#
# | Name        | Completion | Streaming | Models          |
# | ----------- | ---------- | --------- | --------------- |
# | `openai`    | ✓          | ✓         | ✓               |
# | `anthropic` | ✓          | —         | ✓ (static list) |
module CosmosLLM
end

require "./cosmos_llm/version"
require "./cosmos_llm/error"
require "./cosmos_llm/types"
require "./cosmos_llm/config"
require "./cosmos_llm/providers"
require "./cosmos_llm/client"
