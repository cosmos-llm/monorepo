require "./spec_helper"

describe CosmosLLM::Config do
  it "defaults to the openai provider" do
    CosmosLLM::Config.new(load_env: false).default_provider.should eq("openai")
  end

  it "sets and reads an api key" do
    config = CosmosLLM::Config.new(load_env: false)
    config.set_api_key("anthropic", "sk-ant-test")

    config.api_key("anthropic").should eq("sk-ant-test")
  end

  it "sets and reads a model" do
    config = CosmosLLM::Config.new(load_env: false)
    config.set_model("openai", "gpt-4o")

    config.model("openai").should eq("gpt-4o")
  end

  it "returns nil for an unconfigured provider" do
    config = CosmosLLM::Config.new(load_env: false)

    config.api_key("unknown").should be_nil
    config.model("unknown").should be_nil
    config.provider("unknown").should be_nil
  end

  it "overrides the default provider" do
    config = CosmosLLM::Config.new(load_env: false)
    config.default_provider = "anthropic"

    config.default_provider.should eq("anthropic")
  end

  it "loads CLLM__ variables from the environment" do
    with_clean_env do
      ENV["CLLM__OPENAI__API_KEY"] = "sk-from-env"
      ENV["CLLM__OPENAI__MODEL"] = "gpt-4o"
      ENV["CLLM__OPENAI__ORG_ID"] = "org-123"

      config = CosmosLLM::Config.new

      config.api_key("openai").should eq("sk-from-env")
      config.model("openai").should eq("gpt-4o")
      config.provider("openai").not_nil!.extra["org_id"].should eq("org-123")
    end
  end

  it "skips environment loading when asked" do
    with_clean_env do
      ENV["CLLM__OPENAI__API_KEY"] = "sk-from-env"

      CosmosLLM::Config.new(load_env: false).api_key("openai").should be_nil
    end
  end
end
