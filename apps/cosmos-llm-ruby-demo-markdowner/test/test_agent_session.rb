# frozen_string_literal: true

require "test_helper"
require "tmpdir"
require "cosmos_llm_markdowner/error"
require "cosmos_llm_markdowner/sandbox"
require "cosmos_llm_markdowner/tools"
require "cosmos_llm_markdowner/agent_session"

class TestAgentSession < Minitest::Test
  # Minimal client stub — no tool calls, just returns text.
  class FakeClient
    attr_reader :model

    def initialize(model = "fake-model")
      @model = model
    end

    def completion(_params)
      FakeResponse.new([{ "type" => "text", "text" => "# Hello\n\nWorld." }], "end_turn")
    end
  end

  # Client that returns one tool_use round then a text response.
  class FakeClientWithToolCall
    attr_reader :model, :call_count, :out_path

    def initialize(out_path)
      @model      = "fake-model"
      @call_count = 0
      @out_path   = out_path
    end

    def completion(_params)
      @call_count += 1
      if @call_count == 1
        FakeResponse.new(
          [
            { "type" => "text",     "text" => "Let me write that." },
            { "type" => "tool_use", "id" => "tu_01", "name" => "write_file",
              "input" => { "path" => @out_path, "content" => "# Written" } }
          ],
          "tool_use"
        )
      else
        FakeResponse.new([{ "type" => "text", "text" => "Done." }], "end_turn")
      end
    end
  end

  class FakeResponse
    attr_reader :raw_response

    def initialize(content, stop_reason)
      @raw_response = { "content" => content, "stop_reason" => stop_reason }
    end

    def tool_use?
      @raw_response["stop_reason"] == "tool_use"
    end

    def tool_calls
      (@raw_response["content"] || [])
        .select { |b| b["type"] == "tool_use" }
        .map    { |b| FakeToolCall.new(b) }
    end
  end

  FakeToolCall = Struct.new(:id, :name, :input) do
    def initialize(block)
      super(block["id"], block["name"], block["input"] || {})
    end
  end

  def setup
    @tmpdir = Dir.mktmpdir
  end

  def teardown
    FileUtils.rm_rf(@tmpdir)
  end

  def session_with(client, sandbox: nil)
    sandbox ||= CosmosLlmMarkdowner::Sandbox.new
    session = CosmosLlmMarkdowner::AgentSession.new(sandbox: sandbox)
    session.instance_variable_set(:@client, client)
    session
  end

  def test_run_returns_text_response
    session = session_with(FakeClient.new)
    reply = session.run("Write a hello doc")
    assert_equal "# Hello\n\nWorld.", reply
  end

  def test_history_grows_after_run
    session = session_with(FakeClient.new)
    session.run("hello")
    assert_equal 2, session.history.size
    assert_equal "user",      session.history.first[:role]
    assert_equal "assistant", session.history.last[:role]
  end

  def test_history_is_a_copy
    session = session_with(FakeClient.new)
    session.run("hello")
    h = session.history
    h.clear
    assert_equal 2, session.history.size
  end

  def test_tool_use_loop_executes_write
    out_path = File.join(@tmpdir, "out.md")
    sandbox  = CosmosLlmMarkdowner::Sandbox.new(allowed_writes: [out_path])

    client  = FakeClientWithToolCall.new(out_path)
    session = session_with(client, sandbox: sandbox)

    # Patch the tool map so write_file resolves to out_path
    write_tool = CosmosLlmMarkdowner::Tools.for_sandbox(sandbox).find { |t| t.name == :write_file }
    session.instance_variable_set(:@tool_map, { "write_file" => write_tool })

    reply = session.run("Write out.md")

    assert_equal "Done.", reply
    assert_equal 2, client.call_count
    assert File.exist?(out_path), "expected file to be written"
    assert_equal "# Written", File.read(out_path)
  end

  def test_provider_error_wraps_client_failure
    bad_client = Class.new do
      def model = "x"
      def completion(_) = raise("boom")
    end.new

    session = session_with(bad_client)
    err = assert_raises(CosmosLlmMarkdowner::ProviderError) { session.run("hi") }
    assert_match "boom", err.message
  end
end
