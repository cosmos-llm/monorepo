module CosmosLLM
  # Base class for every error raised by this shard.
  #
  # Rescue `CosmosLLM::Error` to catch any library failure; rescue a
  # subclass to handle a specific condition.
  #
  # ```
  # begin
  #   client.complete("hello")
  # rescue ex : CosmosLLM::RateLimitError
  #   sleep 5.seconds
  # rescue ex : CosmosLLM::Error
  #   STDERR.puts ex.message
  # end
  # ```
  class Error < Exception
  end

  # The API key is missing or was rejected by the provider.
  class AuthenticationError < Error
  end

  # The provider returned a 429 Too Many Requests response.
  class RateLimitError < Error
  end

  # The request was malformed or contained invalid parameters.
  class InvalidRequestError < Error
  end

  # The requested resource (e.g. model) was not found.
  class NotFoundError < Error
  end

  # The provider's server returned a 5xx error.
  class ServerError < Error
  end

  # The provider name is not recognised.
  class UnsupportedProviderError < Error
  end

  # A configuration value is missing or invalid.
  class ConfigurationError < Error
  end

  # The account has insufficient quota or credits.
  class InsufficientQuotaError < Error
  end

  # The response from the provider could not be parsed.
  class InvalidResponseError < Error
  end

  # An error occurred during a streaming response.
  class StreamingError < Error
  end
end
