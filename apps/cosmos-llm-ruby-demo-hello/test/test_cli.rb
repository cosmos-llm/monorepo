# frozen_string_literal: true

require "test_helper"
require "cosmos_llm_hello/error"
require "cosmos_llm_hello/chat_session"
require "cosmos_llm_hello/cli"

class TestCLI < Minitest::Test
  def test_parse_defaults
    cli = CosmosLlmHello::CLI.new([])
    opts = cli.instance_variable_get(:@options)
    assert_equal CosmosLlmHello::ChatSession::DEFAULT_PROVIDER, opts[:provider]
    assert_equal CosmosLlmHello::ChatSession::DEFAULT_MODEL, opts[:model]
    assert_nil opts[:system_prompt]
  end

  def test_parse_provider_flag
    cli = CosmosLlmHello::CLI.new(["-p", "openai"])
    assert_equal "openai", cli.instance_variable_get(:@options)[:provider]
  end

  def test_parse_model_flag
    cli = CosmosLlmHello::CLI.new(["-m", "gpt-4o"])
    assert_equal "gpt-4o", cli.instance_variable_get(:@options)[:model]
  end

  def test_parse_system_prompt_flag
    cli = CosmosLlmHello::CLI.new(["-s", "Be helpful."])
    assert_equal "Be helpful.", cli.instance_variable_get(:@options)[:system_prompt]
  end

  def test_quit_commands_constant
    assert_includes CosmosLlmHello::CLI::QUIT_COMMANDS, "exit"
    assert_includes CosmosLlmHello::CLI::QUIT_COMMANDS, "quit"
    assert_includes CosmosLlmHello::CLI::QUIT_COMMANDS, "q"
  end
end
