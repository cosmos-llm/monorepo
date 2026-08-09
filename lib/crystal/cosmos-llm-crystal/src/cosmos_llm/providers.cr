require "./providers/base"
require "./providers/anthropic"
require "./providers/openai"

module CosmosLLM
  # Provider implementations and the name-to-provider resolver.
  module Providers
    # Resolves a provider name to a `Base` instance.
    #
    # Lookup is case-insensitive. When *api_key* is `nil`, the provider falls
    # back to its own environment variables.
    #
    # Raises `UnsupportedProviderError` when *name* is not recognised.
    #
    # ```
    # provider = CosmosLLM::Providers.resolve("openai", "sk-test")
    # provider.supports_streaming? # => true
    # ```
    def self.resolve(name : String, api_key : String? = nil) : Base
      case name.downcase
      when "openai"    then OpenAI.new(api_key)
      when "anthropic" then Anthropic.new(api_key)
      else                  raise UnsupportedProviderError.new("unsupported provider: #{name}")
      end
    end

    # Returns the names accepted by `.resolve`.
    def self.available : Array(String)
      ["openai", "anthropic"]
    end
  end
end
