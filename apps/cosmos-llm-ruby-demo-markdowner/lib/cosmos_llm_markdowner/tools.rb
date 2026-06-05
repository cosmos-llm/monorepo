# frozen_string_literal: true

require "cosmos/llm/tool"
require_relative "sandbox"

module CosmosLlmMarkdowner
  # Builds the sandboxed tool definitions passed to the LLM.
  module Tools
    # @param sandbox [Sandbox]
    # @return [Array<Cosmos::Llm::Tool::Definition>]
    def self.for_sandbox(sandbox)
      [read_tool(sandbox), write_tool(sandbox)]
    end

    # @param tools [Array<Cosmos::Llm::Tool::Definition>]
    # @return [Array<Hash>] Anthropic-format tool schemas
    def self.to_anthropic_schemas(tools)
      tools.map(&:to_anthropic_schema)
    end

    private_class_method def self.read_tool(sandbox)
      Cosmos::Llm::Tool.define(:read_file, register: false) do
        description "Read a file from the filesystem. Only files explicitly allowed with --in may be read."

        parameter :path,
                  type: :string,
                  required: true,
                  description: "Absolute or relative path to the file to read."

        execute do |params|
          sandbox.read(params[:path])
        rescue CosmosLlmMarkdowner::FilesystemError => e
          { error: e.message }
        end
      end
    end

    private_class_method def self.write_tool(sandbox)
      Cosmos::Llm::Tool.define(:write_file, register: false) do
        description "Write content to a file. Only paths allowed with --out or " \
                    "inside directories allowed with --out-dir may be written."

        parameter :path,
                  type: :string,
                  required: true,
                  description: "Destination file path."

        parameter :content,
                  type: :string,
                  required: true,
                  description: "Full markdown content to write."

        execute do |params|
          sandbox.write(params[:path], params[:content])
          { written: params[:path] }
        rescue CosmosLlmMarkdowner::FilesystemError => e
          { error: e.message }
        end
      end
    end
  end
end
