# frozen_string_literal: true

module CosmosLlmHello
  class Error < StandardError; end
  class ConfigurationError < Error; end
  class ProviderError < Error; end
end
