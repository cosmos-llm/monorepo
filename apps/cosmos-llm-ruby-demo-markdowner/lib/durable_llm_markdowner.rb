# frozen_string_literal: true

require_relative "cosmos_llm_markdowner/version"

module CosmosLlmMarkdowner
  autoload :Error, "cosmos_llm_markdowner/error"
  autoload :ConfigurationError, "cosmos_llm_markdowner/error"
  autoload :ProviderError,      "cosmos_llm_markdowner/error"
  autoload :FilesystemError,    "cosmos_llm_markdowner/error"
  autoload :Sandbox,      "cosmos_llm_markdowner/sandbox"
  autoload :Tools,        "cosmos_llm_markdowner/tools"
  autoload :AgentSession, "cosmos_llm_markdowner/agent_session"
  autoload :CLI,          "cosmos_llm_markdowner/cli"
end
