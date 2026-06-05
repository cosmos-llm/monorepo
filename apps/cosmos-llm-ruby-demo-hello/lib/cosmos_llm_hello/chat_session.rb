# frozen_string_literal: true

require "cosmos/llm"
require "cosmos/llm/client"

module CosmosLlmHello
  # Manages a single interactive chat session with an LLM provider.
  class ChatSession
    DEFAULT_PROVIDER = "anthropic"
    DEFAULT_MODEL    = "claude-haiku-4-5-20251001"

    # @param provider [String] cosmos-llm provider name (e.g. "anthropic", "openai")
    # @param model [String] model identifier passed to the provider
    # @param system_prompt [String, nil] optional system message prepended to every request
    def initialize(provider: DEFAULT_PROVIDER, model: DEFAULT_MODEL, system_prompt: nil)
      @history       = []
      @system_prompt = system_prompt
      @client        = build_client(provider, model)
    end

    # Send a user message and return the assistant reply.
    #
    # @param message [String] user input
    # @return [String] assistant response text
    # @raise [CosmosLlmHello::ProviderError] if the LLM call fails
    def chat(message)
      @history << { role: "user", content: message }
      params = build_params
      response = @client.completion(params)
      reply = response.choices.first.message.content
      @history << { role: "assistant", content: reply }
      reply
    rescue StandardError => e
      raise ProviderError, "LLM request failed: #{e.message}"
    end

    # Clear conversation history (system prompt is preserved).
    #
    # @return [void]
    def reset
      @history.clear
    end

    # @return [Array<Hash>] a copy of the conversation history
    def history
      @history.dup
    end

    private

    def build_client(provider, model)
      Cosmos::Llm::Client.new(provider, model: model)
    end

    def build_params
      messages = []
      messages << { role: "system", content: @system_prompt } if @system_prompt
      messages.concat(@history)
      { messages: messages }
    end
  end
end
