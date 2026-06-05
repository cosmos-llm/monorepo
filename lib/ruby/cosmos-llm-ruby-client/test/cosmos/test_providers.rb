# frozen_string_literal: true

require 'minitest/autorun'
require 'mocha/minitest'
require 'cosmos/llm/providers'
require 'cosmos/llm/providers/huggingface'

class TestProviders < Minitest::Test
  def setup
    @original_providers = Cosmos::Llm::Providers.instance_variable_get(:@providers)
    Cosmos::Llm::Providers.instance_variable_set(:@providers, nil)
  end

  def teardown
    Cosmos::Llm::Providers.instance_variable_set(:@providers, @original_providers)
  end

  def test_providers
    Cosmos::Llm::Providers.stubs(:constants).returns(%i[OpenAI Anthropic Huggingface Base])
    Cosmos::Llm::Providers.const_get(:OpenAI).stubs(:name).returns('Cosmos::Llm::Providers::OpenAI')
    Cosmos::Llm::Providers.const_get(:Anthropic).stubs(:name).returns('Cosmos::Llm::Providers::Anthropic')
    Cosmos::Llm::Providers.const_get(:Huggingface).stubs(:name).returns('Cosmos::Llm::Providers::Huggingface')
    Cosmos::Llm::Providers.const_get(:Base).stubs(:name).returns('Cosmos::Llm::Providers::Base')

    providers = Cosmos::Llm::Providers.providers
    assert_includes providers, :openai
    assert_includes providers, :anthropic
    assert_includes providers, :huggingface
    refute_includes providers, :base
  end

  def test_model_ids
    Cosmos::Llm::Providers.stubs(:providers).returns(%i[openai anthropic huggingface])
    Cosmos::Llm::Providers::OpenAI.stubs(:models).returns(['gpt-3.5-turbo', 'gpt-4'])
    Cosmos::Llm::Providers::Anthropic.stubs(:models).returns(['claude-2.1', 'claude-instant-1.2'])
    Cosmos::Llm::Providers::Huggingface.stubs(:models).returns(%w[gpt2 bert-base-uncased])

    model_ids = Cosmos::Llm::Providers.model_ids
    assert_includes model_ids, 'gpt-3.5-turbo'
    assert_includes model_ids, 'gpt-4'
    assert_includes model_ids, 'claude-2.1'
    assert_includes model_ids, 'claude-instant-1.2'
    assert_includes model_ids, 'gpt2'
    assert_includes model_ids, 'bert-base-uncased'
  end

  def test_model_id_to_provider
    Cosmos::Llm::Providers.stubs(:providers).returns(%i[openai anthropic huggingface])
    Cosmos::Llm::Providers::OpenAI.stubs(:models).returns(['gpt-3.5-turbo', 'gpt-4'])
    Cosmos::Llm::Providers::Anthropic.stubs(:models).returns(['claude-2.1', 'claude-instant-1.2'])
    Cosmos::Llm::Providers::Huggingface.stubs(:models).returns(%w[gpt2 bert-base-uncased])

    assert_equal Cosmos::Llm::Providers::OpenAI, Cosmos::Llm::Providers.model_id_to_provider('gpt-3.5-turbo')
    assert_equal Cosmos::Llm::Providers::Anthropic, Cosmos::Llm::Providers.model_id_to_provider('claude-2.1')
    assert_equal Cosmos::Llm::Providers::Huggingface, Cosmos::Llm::Providers.model_id_to_provider('gpt2')
    assert_nil Cosmos::Llm::Providers.model_id_to_provider('nonexistent-model')
  end

  def test_provider_aliases
    assert_equal Cosmos::Llm::Providers::OpenAI, Cosmos::Llm::Providers::Openai
    assert_equal Cosmos::Llm::Providers::Anthropic, Cosmos::Llm::Providers::Claude
    assert_equal Cosmos::Llm::Providers::Anthropic, Cosmos::Llm::Providers::Claude3
  end

  def test_load_all
    # Should not raise an error and return the list of files
    files = Cosmos::Llm::Providers.load_all
    assert_kind_of Array, files
    assert(files.all? { |f| f.end_with?('.rb') })
  end

  def test_provider_class_for
    assert_equal Cosmos::Llm::Providers::OpenAI, Cosmos::Llm::Providers.provider_class_for(:openai)
    assert_equal Cosmos::Llm::Providers::Anthropic, Cosmos::Llm::Providers.provider_class_for(:anthropic)
    assert_equal Cosmos::Llm::Providers::DeepSeek, Cosmos::Llm::Providers.provider_class_for(:deepseek)
    assert_equal Cosmos::Llm::Providers::OpenRouter, Cosmos::Llm::Providers.provider_class_for(:openrouter)
    assert_equal Cosmos::Llm::Providers::AzureOpenai, Cosmos::Llm::Providers.provider_class_for(:azureopenai)
    assert_equal Cosmos::Llm::Providers::Opencode, Cosmos::Llm::Providers.provider_class_for(:opencode)
  end
end
