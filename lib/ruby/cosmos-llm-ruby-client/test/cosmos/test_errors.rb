# frozen_string_literal: true

require 'minitest/autorun'
require 'cosmos/llm/errors'

class TestErrors < Minitest::Test
  def test_error_hierarchy
    # Test that all error classes inherit from the base Error class
    assert_kind_of StandardError, Cosmos::Llm::Error.new
    assert_kind_of Cosmos::Llm::Error, Cosmos::Llm::APIError.new
    assert_kind_of Cosmos::Llm::Error, Cosmos::Llm::RateLimitError.new
    assert_kind_of Cosmos::Llm::Error, Cosmos::Llm::AuthenticationError.new
    assert_kind_of Cosmos::Llm::Error, Cosmos::Llm::InvalidRequestError.new
    assert_kind_of Cosmos::Llm::Error, Cosmos::Llm::ResourceNotFoundError.new
    assert_kind_of Cosmos::Llm::Error, Cosmos::Llm::TimeoutError.new
    assert_kind_of Cosmos::Llm::Error, Cosmos::Llm::ServerError.new
    assert_kind_of Cosmos::Llm::Error, Cosmos::Llm::UnsupportedProviderError.new
    assert_kind_of Cosmos::Llm::Error, Cosmos::Llm::ConfigurationError.new
    assert_kind_of Cosmos::Llm::Error, Cosmos::Llm::ModelNotFoundError.new
    assert_kind_of Cosmos::Llm::Error, Cosmos::Llm::InsufficientQuotaError.new
    assert_kind_of Cosmos::Llm::Error, Cosmos::Llm::InvalidResponseError.new
    assert_kind_of Cosmos::Llm::Error, Cosmos::Llm::NetworkError.new
    assert_kind_of Cosmos::Llm::Error, Cosmos::Llm::StreamingError.new
  end

  def test_error_classes_are_exceptions
    # Test that all error classes are proper exceptions
    assert_raises Cosmos::Llm::APIError do
      raise Cosmos::Llm::APIError.new('API error')
    end

    assert_raises Cosmos::Llm::RateLimitError do
      raise Cosmos::Llm::RateLimitError.new('Rate limit exceeded')
    end

    assert_raises Cosmos::Llm::AuthenticationError do
      raise Cosmos::Llm::AuthenticationError.new('Authentication failed')
    end

    assert_raises Cosmos::Llm::InvalidRequestError do
      raise Cosmos::Llm::InvalidRequestError.new('Invalid request')
    end

    assert_raises Cosmos::Llm::ResourceNotFoundError do
      raise Cosmos::Llm::ResourceNotFoundError.new('Resource not found')
    end

    assert_raises Cosmos::Llm::TimeoutError do
      raise Cosmos::Llm::TimeoutError.new('Request timed out')
    end

    assert_raises Cosmos::Llm::ServerError do
      raise Cosmos::Llm::ServerError.new('Server error')
    end

    assert_raises Cosmos::Llm::UnsupportedProviderError do
      raise Cosmos::Llm::UnsupportedProviderError.new('Unsupported provider')
    end

    assert_raises Cosmos::Llm::ConfigurationError do
      raise Cosmos::Llm::ConfigurationError.new('Configuration error')
    end

    assert_raises Cosmos::Llm::ModelNotFoundError do
      raise Cosmos::Llm::ModelNotFoundError.new('Model not found')
    end

    assert_raises Cosmos::Llm::InsufficientQuotaError do
      raise Cosmos::Llm::InsufficientQuotaError.new('Insufficient quota')
    end

    assert_raises Cosmos::Llm::InvalidResponseError do
      raise Cosmos::Llm::InvalidResponseError.new('Invalid response')
    end

    assert_raises Cosmos::Llm::NetworkError do
      raise Cosmos::Llm::NetworkError.new('Network error')
    end

    assert_raises Cosmos::Llm::StreamingError do
      raise Cosmos::Llm::StreamingError.new('Streaming error')
    end
  end

  def test_error_messages
    # Test that error messages are properly set
    error = Cosmos::Llm::APIError.new('Test message')
    assert_equal 'Test message', error.message

    error = Cosmos::Llm::RateLimitError.new('Rate limit hit')
    assert_equal 'Rate limit hit', error.message
  end

  def test_error_inheritance_chain
    # Test that errors can be rescued as the base Error class
    begin
      raise Cosmos::Llm::APIError.new('API error')
    rescue Cosmos::Llm::Error => e
      assert_equal 'API error', e.message
      assert_instance_of Cosmos::Llm::APIError, e
    end

    begin
      raise Cosmos::Llm::RateLimitError.new('Rate limit')
    rescue Cosmos::Llm::Error => e
      assert_equal 'Rate limit', e.message
      assert_instance_of Cosmos::Llm::RateLimitError, e
    end
  end
end
