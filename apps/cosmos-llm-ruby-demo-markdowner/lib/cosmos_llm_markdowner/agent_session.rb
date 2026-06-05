# frozen_string_literal: true

require "json"
require "cosmos/llm"
require "cosmos/llm/client"
require_relative "error"
require_relative "tools"

module CosmosLlmMarkdowner
  # Runs an agentic loop: sends messages to the LLM, executes tool calls
  # when requested, and feeds results back until the model produces a
  # final text response.
  class AgentSession
    DEFAULT_PROVIDER = "anthropic"
    DEFAULT_MODEL    = "claude-sonnet-4-6"

    BASE_SYSTEM_PROMPT = <<~PROMPT
      You are a markdown writing assistant. Your job is to produce well-structured
      markdown files. When the user asks you to write to a file, use the write_file
      tool. When you need to read input files that were provided, use the read_file
      tool. Always produce clean, valid markdown.
    PROMPT

    # @param sandbox  [Sandbox]
    # @param provider [String]
    # @param model    [String]
    # @param preload  [Array<String>] file paths to read and prepend as context
    def initialize(sandbox:, provider: DEFAULT_PROVIDER, model: DEFAULT_MODEL, preload: [])
      @sandbox  = sandbox
      @client   = Cosmos::Llm::Client.new(provider, model: model)
      @tools    = Tools.for_sandbox(sandbox)
      @tool_map = @tools.to_h { |t| [t.name.to_s, t] }
      @history  = []
      @preload  = preload
    end

    # Send a user message and run the agentic loop until the model stops.
    #
    # @param message [String] user prompt
    # @return [String] final text response from the model
    # @raise [CosmosLlmMarkdowner::ProviderError] on LLM failure
    def run(message)
      @history << { role: "user", content: build_user_content(message) }
      agentic_loop
    rescue StandardError => e
      raise ProviderError, "LLM request failed: #{e.message}"
    end

    # @return [Array<Hash>] copy of conversation history
    def history
      @history.dup
    end

    private

    def build_user_content(message)
      return message if @preload.empty?

      parts = @preload.filter_map do |path|
        content = @sandbox.read(path)
        "### #{path}\n\n#{content}"
      rescue FilesystemError
        nil
      end

      return message if parts.empty?

      "#{parts.join("\n\n---\n\n")}\n\n---\n\n#{message}"
    end

    def agentic_loop
      loop do
        response = call_llm
        assistant_content = response.raw_response["content"] || []

        @history << { role: "assistant", content: assistant_content }

        break extract_text(assistant_content) unless response.tool_use?

        tool_results = execute_tool_calls(response.tool_calls)
        @history << { role: "user", content: tool_results }
      end
    end

    def call_llm
      params = {
        "model" => extract_model,
        "max_tokens" => 4096,
        "system" => system_prompt,
        "messages" => @history,
        "tools" => Tools.to_anthropic_schemas(@tools)
      }
      @client.completion(params)
    end

    def system_prompt
      prompt = BASE_SYSTEM_PROMPT.dup

      writable = @sandbox.writable_files.map(&:to_s) +
                 @sandbox.writable_dirs.map(&:to_s)

      if writable.any?
        prompt += "\nYou are allowed to write to these paths: #{writable.join(", ")}." \
                  " Use one of these exact paths when calling write_file."
      end

      prompt
    end

    def execute_tool_calls(tool_calls)
      tool_calls.map do |tc|
        tool = @tool_map[tc.name]
        result = if tool
                   tool.call(tc.input.transform_keys(&:to_sym))
                 else
                   { error: "Unknown tool: #{tc.name}" }
                 end

        {
          type: "tool_result",
          tool_use_id: tc.id,
          content: result.is_a?(String) ? result : result.to_json
        }
      end
    end

    def extract_text(content_blocks)
      blocks = [content_blocks].flatten
      blocks.select { |b| b.is_a?(Hash) && b["type"] == "text" }
            .map { |b| b["text"] }
            .join("\n")
    end

    def extract_model
      @client.model || DEFAULT_MODEL
    end
  end
end
