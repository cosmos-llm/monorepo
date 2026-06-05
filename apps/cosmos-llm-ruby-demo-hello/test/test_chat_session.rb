# frozen_string_literal: true

require "test_helper"
require "cosmos_llm_hello/error"
require "cosmos_llm_hello/chat_session"

class TestChatSession < Minitest::Test
  # Minimal stub matching Cosmos::Llm::Client's interface
  class FakeClient
    FakeMessage  = Struct.new(:content)
    FakeChoice   = Struct.new(:message)
    FakeResponse = Struct.new(:choices)

    def completion(params)
      messages = params[:messages]
      last_user = messages.reverse.find { |m| m[:role] == "user" }
      FakeResponse.new([FakeChoice.new(FakeMessage.new("echo: #{last_user[:content]}"))])
    end
  end

  def setup
    @session = CosmosLlmHello::ChatSession.new
    @session.instance_variable_set(:@client, FakeClient.new)
  end

  def test_chat_returns_reply
    reply = @session.chat("hello")
    assert_equal "echo: hello", reply
  end

  def test_history_grows_with_each_turn
    @session.chat("one")
    @session.chat("two")
    assert_equal 4, @session.history.size
  end

  def test_history_is_a_copy
    @session.chat("hello")
    h = @session.history
    h.clear
    assert_equal 2, @session.history.size
  end

  def test_reset_clears_history
    @session.chat("hello")
    @session.reset
    assert_empty @session.history
  end

  def test_system_prompt_prepended
    session = CosmosLlmHello::ChatSession.new(system_prompt: "Be terse.")
    session.instance_variable_set(:@client, FakeClient.new)

    # Verify system prompt appears as first message in the params passed to the provider
    session.chat("hi")
    built = session.__send__(:build_params)
    assert_equal "system", built[:messages].first[:role]
  end

  def test_provider_error_wraps_client_failure
    bad_client = Class.new do
      def completion(_params)
        raise "network down"
      end
    end.new

    @session.instance_variable_set(:@client, bad_client)
    err = assert_raises(CosmosLlmHello::ProviderError) { @session.chat("hi") }
    assert_match "network down", err.message
  end
end
