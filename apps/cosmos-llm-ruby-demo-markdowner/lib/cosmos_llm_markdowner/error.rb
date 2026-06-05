# frozen_string_literal: true

module CosmosLlmMarkdowner
  class Error < StandardError; end
  class ConfigurationError < Error; end
  class ProviderError < Error; end
  class FilesystemError < Error; end
end
