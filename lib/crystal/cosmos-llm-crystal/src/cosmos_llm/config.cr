module CosmosLLM
  # Per-provider settings: API key, default model, and arbitrary extra fields.
  class ProviderConfig
    # API key for this provider.
    property api_key : String?

    # Default model name to use when none is specified in the request.
    property model : String?

    # Additional provider-specific settings.
    property extra : Hash(String, String)

    def initialize(
      @api_key : String? = nil,
      @model : String? = nil,
      @extra : Hash(String, String) = {} of String => String,
    )
    end
  end

  # Global library configuration.
  #
  # Holds per-provider settings and a default provider name. Configuration can
  # be built programmatically or loaded from environment variables
  # automatically.
  #
  # ## Environment variables
  #
  # The library reads `CLLM__<PROVIDER>__<SETTING>` variables on construction:
  #
  # ```text
  # CLLM__OPENAI__API_KEY=sk-...
  # CLLM__ANTHROPIC__API_KEY=sk-ant-...
  # CLLM__OPENAI__MODEL=gpt-4o
  # ```
  #
  # ```
  # config = CosmosLLM::Config.new
  # config.set_api_key("openai", "sk-...")
  # config.default_provider = "openai"
  # ```
  class Config
    # Name of the provider used when none is given explicitly.
    property default_provider : String

    @providers : Hash(String, ProviderConfig)

    # Creates a new `Config` and loads settings from environment variables.
    #
    # Pass `load_env: false` to skip environment loading, which is useful in
    # tests that need a hermetic configuration.
    def initialize(@default_provider : String = "openai", load_env : Bool = true)
      @providers = {} of String => ProviderConfig
      load_from_env if load_env
    end

    # Sets the API key for a provider.
    #
    # ```
    # config.set_api_key("openai", "sk-test")
    # config.api_key("openai") # => "sk-test"
    # ```
    def set_api_key(provider : String, key : String) : Nil
      provider_config(provider).api_key = key
    end

    # Returns the API key for a provider, or `nil` when unset.
    def api_key(provider : String) : String?
      @providers[provider]?.try(&.api_key)
    end

    # Sets the default model for a provider.
    #
    # ```
    # config.set_model("openai", "gpt-4o")
    # config.model("openai") # => "gpt-4o"
    # ```
    def set_model(provider : String, model : String) : Nil
      provider_config(provider).model = model
    end

    # Returns the default model for a provider, or `nil` when unset.
    def model(provider : String) : String?
      @providers[provider]?.try(&.model)
    end

    # Returns the full `ProviderConfig` for a provider, or `nil` when the
    # provider has no settings.
    def provider(name : String) : ProviderConfig?
      @providers[name]?
    end

    # Returns the `ProviderConfig` for *name*, creating it when absent.
    def provider_config(name : String) : ProviderConfig
      @providers[name] ||= ProviderConfig.new
    end

    private def load_from_env : Nil
      ENV.each do |key, value|
        next unless key.starts_with?("CLLM__")

        parts = key.split("__", 3)
        next if parts.size < 3

        pc = provider_config(parts[1].downcase)
        case parts[2].downcase
        when "api_key" then pc.api_key = value
        when "model"   then pc.model = value
        else                pc.extra[parts[2].downcase] = value
        end
      end
    end
  end
end
