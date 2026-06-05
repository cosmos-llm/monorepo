# frozen_string_literal: true

require "test_helper"
require "cosmos_llm_markdowner/error"
require "cosmos_llm_markdowner/sandbox"
require "cosmos_llm_markdowner/tools"
require "cosmos_llm_markdowner/agent_session"
require "cosmos_llm_markdowner/cli"

class TestCLI < Minitest::Test
  def test_parse_defaults
    cli  = CosmosLlmMarkdowner::CLI.new([])
    opts = cli.instance_variable_get(:@options)
    assert_equal CosmosLlmMarkdowner::AgentSession::DEFAULT_PROVIDER, opts[:provider]
    assert_equal CosmosLlmMarkdowner::AgentSession::DEFAULT_MODEL,    opts[:model]
    assert_empty opts[:in_files]
    assert_empty opts[:out_files]
    assert_empty opts[:out_dirs]
  end

  def test_parse_provider_flag
    cli = CosmosLlmMarkdowner::CLI.new(["-p", "openai"])
    assert_equal "openai", cli.instance_variable_get(:@options)[:provider]
  end

  def test_parse_model_flag
    cli = CosmosLlmMarkdowner::CLI.new(["-m", "gpt-4o"])
    assert_equal "gpt-4o", cli.instance_variable_get(:@options)[:model]
  end

  def test_parse_in_flag
    cli = CosmosLlmMarkdowner::CLI.new(["--in", "/tmp/foo.md"])
    assert_includes cli.instance_variable_get(:@options)[:in_files], File.expand_path("/tmp/foo.md")
  end

  def test_parse_out_flag
    cli = CosmosLlmMarkdowner::CLI.new(["--out", "/tmp/out.md"])
    assert_includes cli.instance_variable_get(:@options)[:out_files], File.expand_path("/tmp/out.md")
  end

  def test_parse_out_dir_flag
    cli = CosmosLlmMarkdowner::CLI.new(["--out-dir", "/tmp/docs"])
    assert_includes cli.instance_variable_get(:@options)[:out_dirs], File.expand_path("/tmp/docs")
  end

  def test_parse_out_dir_multiple
    cli = CosmosLlmMarkdowner::CLI.new(["--out-dir", "/tmp/a", "--out-dir", "/tmp/b"])
    dirs = cli.instance_variable_get(:@options)[:out_dirs]
    assert_equal 2, dirs.size
  end

  def test_parse_prompt_flag
    cli = CosmosLlmMarkdowner::CLI.new(["--prompt", "Write a readme"])
    assert_equal "Write a readme", cli.instance_variable_get(:@prompt)
  end
end
