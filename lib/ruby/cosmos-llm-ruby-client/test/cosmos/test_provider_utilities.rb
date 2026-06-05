# frozen_string_literal: true

require_relative '../test_helper'
require 'minitest/autorun'
require 'mocha/minitest'
require 'cosmos/llm/provider_utilities'
require 'cosmos/llm/providers'

class TestProviderUtilities < Minitest::Test
  def setup
    @original_config = Cosmos::Llm.configuration
    Cosmos::Llm.configuration = Cosmos::Llm::Configuration.new
  end

  def teardown
    Cosmos::Llm.configuration = @original_config
  end

  def test_available_providers
    providers = Cosmos::Llm::ProviderUtilities.available_providers
    assert_kind_of Array, providers
    refute_empty providers
    # Providers are returned as strings, not symbols
    assert_includes providers, 'openai'
    assert_includes providers, 'anthropic'
  end

  def test_provider_for_model_openai
    # Stub the entire model_id_to_provider method to avoid network calls
    Cosmos::Llm::Providers.stubs(:model_id_to_provider).with('gpt-4').returns(Cosmos::Llm::Providers::OpenAI)

    provider = Cosmos::Llm::ProviderUtilities.provider_for_model('gpt-4')
    assert_equal Cosmos::Llm::Providers::OpenAI, provider
  end

  def test_provider_for_model_anthropic
    # Stub the entire model_id_to_provider method to avoid network calls
    Cosmos::Llm::Providers.stubs(:model_id_to_provider).with('claude-3-opus-20240229').returns(Cosmos::Llm::Providers::Anthropic)

    provider = Cosmos::Llm::ProviderUtilities.provider_for_model('claude-3-opus-20240229')
    assert_equal Cosmos::Llm::Providers::Anthropic, provider
  end

  def test_provider_for_model_google
    # Stub the entire model_id_to_provider method to avoid network calls
    Cosmos::Llm::Providers.stubs(:model_id_to_provider).with('gemini-pro').returns(Cosmos::Llm::Providers::Google)

    provider = Cosmos::Llm::ProviderUtilities.provider_for_model('gemini-pro')
    assert_equal Cosmos::Llm::Providers::Google, provider
  end

  def test_provider_for_model_unknown
    # Stub the entire model_id_to_provider method to avoid network calls
    Cosmos::Llm::Providers.stubs(:model_id_to_provider).with('unknown-model-xyz').returns(nil)

    provider = Cosmos::Llm::ProviderUtilities.provider_for_model('unknown-model-xyz')
    assert_nil provider
  end

  def test_models_for_provider_returns_array
    Cosmos::Llm.stubs(:models).returns(['gpt-4', 'gpt-3.5-turbo'])
    models = Cosmos::Llm::ProviderUtilities.models_for_provider(:openai)

    assert_kind_of Array, models
    assert_includes models, 'gpt-4'
    assert_includes models, 'gpt-3.5-turbo'
  end

  def test_models_for_provider_handles_errors
    Cosmos::Llm.stubs(:models).raises(StandardError.new('API error'))
    models = Cosmos::Llm::ProviderUtilities.models_for_provider(:openai)

    assert_equal [], models
  end

  def test_supports_capability_streaming
    mock_provider = mock('provider')
    mock_provider.stubs(:stream?).returns(true)
    mock_provider.stubs(:respond_to?).with(:stream?).returns(true)

    Cosmos::Llm::Providers.stubs(:provider_class_for).returns(mock('class').tap do |klass|
      klass.stubs(:new).returns(mock_provider)
    end)

    result = Cosmos::Llm::ProviderUtilities.supports_capability?(:openai, :streaming)
    assert result
  end

  def test_supports_capability_embeddings
    mock_provider = mock('provider')
    mock_provider.stubs(:respond_to?).with(:embedding).returns(true)

    Cosmos::Llm::Providers.stubs(:provider_class_for).returns(mock('class').tap do |klass|
      klass.stubs(:new).returns(mock_provider)
    end)

    result = Cosmos::Llm::ProviderUtilities.supports_capability?(:openai, :embeddings)
    assert result
  end

  def test_supports_capability_chat
    mock_provider = mock('provider')
    mock_provider.stubs(:respond_to?).with(:completion).returns(true)

    Cosmos::Llm::Providers.stubs(:provider_class_for).returns(mock('class').tap do |klass|
      klass.stubs(:new).returns(mock_provider)
    end)

    result = Cosmos::Llm::ProviderUtilities.supports_capability?(:openai, :chat)
    assert result
  end

  def test_supports_capability_completion
    mock_provider = mock('provider')
    mock_provider.stubs(:respond_to?).with(:completion).returns(true)

    Cosmos::Llm::Providers.stubs(:provider_class_for).returns(mock('class').tap do |klass|
      klass.stubs(:new).returns(mock_provider)
    end)

    result = Cosmos::Llm::ProviderUtilities.supports_capability?(:openai, :completion)
    assert result
  end

  def test_supports_capability_unknown
    mock_provider = mock('provider')

    Cosmos::Llm::Providers.stubs(:provider_class_for).returns(mock('class').tap do |klass|
      klass.stubs(:new).returns(mock_provider)
    end)

    result = Cosmos::Llm::ProviderUtilities.supports_capability?(:openai, :unknown_capability)
    refute result
  end

  def test_supports_capability_handles_errors
    Cosmos::Llm::Providers.stubs(:provider_class_for).raises(StandardError.new('Provider not found'))

    result = Cosmos::Llm::ProviderUtilities.supports_capability?(:invalid, :streaming)
    refute result
  end

  def test_providers_with_capability_streaming
    Cosmos::Llm::ProviderUtilities.stubs(:available_providers).returns(%i[openai anthropic google])
    Cosmos::Llm::ProviderUtilities.stubs(:supports_capability?).with(:openai, :streaming).returns(true)
    Cosmos::Llm::ProviderUtilities.stubs(:supports_capability?).with(:anthropic, :streaming).returns(true)
    Cosmos::Llm::ProviderUtilities.stubs(:supports_capability?).with(:google, :streaming).returns(false)

    providers = Cosmos::Llm::ProviderUtilities.providers_with_capability(:streaming)

    assert_includes providers, :openai
    assert_includes providers, :anthropic
    refute_includes providers, :google
  end

  def test_compare_models
    Cosmos::Llm::ProviderUtilities.stubs(:provider_for_model).with('gpt-4').returns(Cosmos::Llm::Providers::OpenAI)
    Cosmos::Llm::ProviderUtilities.stubs(:provider_for_model).with('claude-3-opus-20240229').returns(Cosmos::Llm::Providers::Anthropic)
    Cosmos::Llm::ProviderUtilities.stubs(:supports_capability?).with(Cosmos::Llm::Providers::OpenAI, :streaming).returns(true)
    Cosmos::Llm::ProviderUtilities.stubs(:supports_capability?).with(Cosmos::Llm::Providers::Anthropic, :streaming).returns(true)

    comparison = Cosmos::Llm::ProviderUtilities.compare_models(['gpt-4', 'claude-3-opus-20240229'])

    assert_equal 2, comparison.length
    assert_equal 'gpt-4', comparison[0][:model]
    assert_equal Cosmos::Llm::Providers::OpenAI, comparison[0][:provider]
    assert comparison[0][:streaming]

    assert_equal 'claude-3-opus-20240229', comparison[1][:model]
    assert_equal Cosmos::Llm::Providers::Anthropic, comparison[1][:provider]
    assert comparison[1][:streaming]
  end

  def test_compare_models_with_unknown_model
    Cosmos::Llm::ProviderUtilities.stubs(:provider_for_model).with('unknown-model').returns(nil)

    comparison = Cosmos::Llm::ProviderUtilities.compare_models(['unknown-model'])

    assert_equal 1, comparison.length
    assert_equal 'unknown-model', comparison[0][:model]
    assert_nil comparison[0][:provider]
    refute comparison[0][:streaming]
  end

  def test_fallback_chain_creates_clients
    model_map = { openai: 'gpt-4', anthropic: 'claude-3-opus-20240229' }

    clients = Cosmos::Llm::ProviderUtilities.fallback_chain(%i[openai anthropic], model_map: model_map)

    assert_equal 2, clients.length
    assert_instance_of Cosmos::Llm::Client, clients[0]
    assert_instance_of Cosmos::Llm::Client, clients[1]
  end

  def test_fallback_chain_handles_errors
    Cosmos::Llm.stubs(:new).with(:invalid, model: nil).raises(StandardError.new('Invalid provider'))
    Cosmos::Llm.stubs(:new).with(:openai, model: 'gpt-4').returns(Cosmos::Llm::Client.new(:openai))

    assert_output(nil, /Failed to create client for invalid/) do
      clients = Cosmos::Llm::ProviderUtilities.fallback_chain(
        %i[invalid openai],
        model_map: { openai: 'gpt-4' }
      )

      assert_equal 1, clients.length
    end
  end

  def test_fallback_chain_without_model_map
    clients = Cosmos::Llm::ProviderUtilities.fallback_chain(%i[openai anthropic])

    assert_equal 2, clients.length
    assert_instance_of Cosmos::Llm::Client, clients[0]
    assert_instance_of Cosmos::Llm::Client, clients[1]
  end

  def test_complete_with_fallback_success_first_provider
    mock_client = mock('client')
    mock_client.expects(:complete).with('Hello').returns('Response')

    Cosmos::Llm.stubs(:new).with(:openai, model: 'gpt-4').returns(mock_client)

    result = Cosmos::Llm::ProviderUtilities.complete_with_fallback(
      'Hello',
      providers: [:openai],
      model_map: { openai: 'gpt-4' }
    )

    assert_equal 'Response', result
  end

  def test_complete_with_fallback_falls_back_on_error
    mock_client1 = mock('client1')
    mock_client1.expects(:complete).with('Hello').raises(StandardError.new('API error'))

    mock_client2 = mock('client2')
    mock_client2.expects(:complete).with('Hello').returns('Fallback response')

    Cosmos::Llm.stubs(:new).with(:openai, model: 'gpt-4').returns(mock_client1)
    Cosmos::Llm.stubs(:new).with(:anthropic, model: 'claude-3-opus-20240229').returns(mock_client2)

    assert_output(nil, /Provider openai failed/) do
      result = Cosmos::Llm::ProviderUtilities.complete_with_fallback(
        'Hello',
        providers: %i[openai anthropic],
        model_map: { openai: 'gpt-4', anthropic: 'claude-3-opus-20240229' }
      )

      assert_equal 'Fallback response', result
    end
  end

  def test_complete_with_fallback_all_fail
    mock_client1 = mock('client1')
    mock_client1.expects(:complete).with('Hello').raises(StandardError.new('Error 1'))

    mock_client2 = mock('client2')
    mock_client2.expects(:complete).with('Hello').raises(StandardError.new('Error 2'))

    Cosmos::Llm.stubs(:new).with(:openai, model: 'gpt-4').returns(mock_client1)
    Cosmos::Llm.stubs(:new).with(:anthropic, model: 'claude-3-opus-20240229').returns(mock_client2)

    assert_output(nil, /Provider openai failed.*Provider anthropic failed/m) do
      result = Cosmos::Llm::ProviderUtilities.complete_with_fallback(
        'Hello',
        providers: %i[openai anthropic],
        model_map: { openai: 'gpt-4', anthropic: 'claude-3-opus-20240229' }
      )

      assert_nil result
    end
  end

  def test_provider_info_with_valid_provider
    Cosmos::Llm::ProviderUtilities.stubs(:supports_capability?).with(:openai, :streaming).returns(true)
    Cosmos::Llm::ProviderUtilities.stubs(:supports_capability?).with(:openai, :embeddings).returns(true)
    Cosmos::Llm::ProviderUtilities.stubs(:supports_capability?).with(:openai, :chat).returns(true)

    info = Cosmos::Llm::ProviderUtilities.provider_info(:openai)

    assert_equal :openai, info[:name]
    assert info[:streaming]
    assert info[:embeddings]
    assert info[:chat]
  end

  def test_provider_info_handles_errors
    Cosmos::Llm::ProviderUtilities.stubs(:supports_capability?).raises(StandardError.new('Provider error'))

    info = Cosmos::Llm::ProviderUtilities.provider_info(:invalid)

    assert_equal :invalid, info[:name]
    assert_equal 'Provider error', info[:error]
  end

  def test_all_provider_info
    Cosmos::Llm::ProviderUtilities.stubs(:available_providers).returns(%i[openai anthropic])
    Cosmos::Llm::ProviderUtilities.stubs(:provider_info).with(:openai).returns(
      { name: :openai, streaming: true, embeddings: true, chat: true }
    )
    Cosmos::Llm::ProviderUtilities.stubs(:provider_info).with(:anthropic).returns(
      { name: :anthropic, streaming: true, embeddings: false, chat: true }
    )

    all_info = Cosmos::Llm::ProviderUtilities.all_provider_info

    assert_equal 2, all_info.length
    assert_equal :openai, all_info[0][:name]
    assert_equal :anthropic, all_info[1][:name]
  end
end
