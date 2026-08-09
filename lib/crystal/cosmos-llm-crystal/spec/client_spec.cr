require "./spec_helper"

describe CosmosLLM::Client do
  it "raises on an unknown provider" do
    expect_raises(CosmosLLM::UnsupportedProviderError, /unknown-provider/) do
      CosmosLLM::Client.new("unknown-provider", "key")
    end
  end

  it "sets the default model" do
    client = CosmosLLM::Client.new("openai", "key").with_model("gpt-4o")

    client.default_model.should eq("gpt-4o")
  end

  it "reports streaming support from the provider" do
    CosmosLLM::Client.new("openai", "key").can_stream?.should be_true
    CosmosLLM::Client.new("anthropic", "key").can_stream?.should be_false
  end

  it "switches providers while keeping the default model" do
    client = CosmosLLM::Client.new("openai", "key").with_model("gpt-4o")
    client.with_provider("anthropic", "sk-ant-key")

    client.provider.should be_a(CosmosLLM::Providers::Anthropic)
    client.default_model.should eq("gpt-4o")
  end

  it "raises a configuration error when completing without a model" do
    client = CosmosLLM::Client.new("openai", "key")

    expect_raises(CosmosLLM::ConfigurationError, /no default model/) do
      client.complete("hello")
    end
  end

  it "raises a configuration error when chatting without a model" do
    client = CosmosLLM::Client.new("openai", "key")

    expect_raises(CosmosLLM::ConfigurationError, /no model specified/) do
      client.chat([CosmosLLM::Message.user("hi")])
    end
  end

  it "builds from a config" do
    config = CosmosLLM::Config.new(load_env: false)
    config.default_provider = "anthropic"
    config.set_api_key("anthropic", "sk-ant-test")
    config.set_model("anthropic", "claude-sonnet-4-6")

    client = CosmosLLM::Client.from_config(config)

    client.provider.should be_a(CosmosLLM::Providers::Anthropic)
    client.default_model.should eq("claude-sonnet-4-6")
  end

  it "lists models from the current provider" do
    client = CosmosLLM::Client.new("anthropic", "key")

    client.models.should contain("claude-sonnet-4-6")
  end
end

describe CosmosLLM::Providers do
  it "resolves known provider names case-insensitively" do
    CosmosLLM::Providers.resolve("OpenAI", "key").should be_a(CosmosLLM::Providers::OpenAI)
    CosmosLLM::Providers.resolve("ANTHROPIC", "key").should be_a(CosmosLLM::Providers::Anthropic)
  end

  it "raises on an unknown name" do
    expect_raises(CosmosLLM::UnsupportedProviderError) do
      CosmosLLM::Providers.resolve("nope")
    end
  end

  it "lists the available providers" do
    CosmosLLM::Providers.available.should eq(["openai", "anthropic"])
  end
end
