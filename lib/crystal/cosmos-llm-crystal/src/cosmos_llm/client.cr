require "./config"
require "./error"
require "./providers"
require "./types"

module CosmosLLM
  # High-level client for interacting with LLM providers.
  #
  # `Client` delegates to the configured provider and exposes both a simple
  # one-shot completion helper (`#complete`) and full control via
  # `#completion`.
  #
  # ```
  # client = CosmosLLM::Client.new("openai", ENV["OPENAI_API_KEY"])
  #   .with_model("gpt-4o")
  #
  # puts client.complete("What is 2 + 2?")
  # ```
  class Client
    # The default model used when a request does not specify one.
    property default_model : String?

    # The provider backing this client.
    getter provider : Providers::Base

    # Creates a `Client` for the named provider with an explicit API key.
    #
    # When *api_key* is `nil`, the provider falls back to its own environment
    # variables.
    #
    # Raises `UnsupportedProviderError` when *provider_name* is not
    # recognised.
    def initialize(provider_name : String, api_key : String? = nil, @default_model : String? = nil)
      @provider = Providers.resolve(provider_name, api_key)
    end

    # Creates a `Client` from a `Config`.
    #
    # The API key and default model are read from *config* for the given
    # provider name. When *provider_name* is omitted, `Config#default_provider`
    # is used.
    #
    # ```
    # config = CosmosLLM::Config.new
    # config.set_api_key("openai", "sk-...")
    # client = CosmosLLM::Client.from_config(config)
    # ```
    def self.from_config(config : Config, provider_name : String? = nil) : self
      name = provider_name || config.default_provider

      new(
        provider_name: name,
        api_key: config.api_key(name),
        default_model: config.model(name),
      )
    end

    # Sets the default model for subsequent requests and returns `self`.
    #
    # ```
    # client = CosmosLLM::Client.new("openai", "sk-test").with_model("gpt-4o")
    # ```
    def with_model(model : String) : self
      @default_model = model
      self
    end

    # Switches to a different provider, preserving the default model, and
    # returns `self`.
    #
    # Raises `UnsupportedProviderError` when *name* is not recognised.
    def with_provider(name : String, api_key : String? = nil) : self
      @provider = Providers.resolve(name, api_key)
      self
    end

    # Returns `true` if the current provider supports streaming.
    def can_stream? : Bool
      @provider.supports_streaming?
    end

    # Sends a plain text prompt and returns the generated text.
    #
    # The prompt is sent as a single `user` message. A default model must be
    # set — via `#with_model` or the `Config` — before calling.
    #
    # Raises `ConfigurationError` when no default model is set, and
    # `InvalidResponseError` when the provider returns no content.
    #
    # ```
    # client.complete("What is the capital of France?")
    # ```
    def complete(prompt : String) : String
      model = @default_model || raise ConfigurationError.new(
        "no default model set; call #with_model or set it in Config"
      )

      response = @provider.completion(
        CompletionRequest.new(model, [Message.user(prompt)])
      )

      response.content || raise InvalidResponseError.new("provider returned no content")
    end

    # Sends a full `CompletionRequest` and returns the provider response.
    #
    # When the request's `model` is empty and a default model is configured,
    # the default is injected automatically.
    #
    # Raises `ConfigurationError` when no model is available.
    #
    # ```
    # req = CosmosLLM::CompletionRequest.new("gpt-4o", [CosmosLLM::Message.user("Hi")])
    #   .with_temperature(0.5)
    # resp = client.completion(req)
    # puts resp.text
    # ```
    def completion(request : CompletionRequest) : CompletionResponse
      if request.model.empty?
        request.model = @default_model || raise ConfigurationError.new("no model specified")
      end

      @provider.completion(request)
    end

    # Sends a chat conversation and returns the provider response.
    #
    # Alias for `#completion` — identical in behaviour.
    def chat(request : CompletionRequest) : CompletionResponse
      completion(request)
    end

    # Builds a `CompletionRequest` from *messages* and sends it.
    #
    # Uses the default model when *model* is not given.
    #
    # ```
    # resp = client.chat([
    #   CosmosLLM::Message.system("You are a pirate."),
    #   CosmosLLM::Message.user("Where is the treasure?"),
    # ])
    # ```
    def chat(
      messages : Array(Message),
      model : String? = nil,
      temperature : Float64? = nil,
      max_tokens : Int32? = nil,
    ) : CompletionResponse
      resolved = model || @default_model || raise ConfigurationError.new("no model specified")

      completion(
        CompletionRequest.new(
          model: resolved,
          messages: messages,
          temperature: temperature,
          max_tokens: max_tokens,
        )
      )
    end

    # Returns the list of models available from the current provider.
    def models : Array(String)
      @provider.models
    end
  end
end
