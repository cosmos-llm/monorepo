# frozen_string_literal: true

require_relative "cosmos_llm_hello/version"

module CosmosLlmHello
  autoload :Error, "cosmos_llm_hello/error"
  autoload :ConfigurationError, "cosmos_llm_hello/error"
  autoload :ProviderError, "cosmos_llm_hello/error"
  autoload :ChatSession,  "cosmos_llm_hello/chat_session"
  autoload :CLI,          "cosmos_llm_hello/cli"
end
