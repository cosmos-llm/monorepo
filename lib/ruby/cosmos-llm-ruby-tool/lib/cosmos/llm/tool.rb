# frozen_string_literal: true

# Main entry point for the Cosmos::Llm::Tool module.
#
# This module provides a comprehensive tool system for Large Language Models,
# enabling function calling, tool registration, execution management, and
# schema generation for various LLM providers.
#
# The module uses Zeitwerk for efficient autoloading of its components.
#
# ## Basic Usage
#
# ```ruby
# require 'cosmos/llm/tool'
#
# # Define a tool
# calculator = Cosmos::Llm::Tool.define(:calculator) do
#   description 'Performs basic arithmetic operations'
#
#   parameter :operation, type: :string, enum: %w[add subtract multiply divide], required: true
#   parameter :a, type: :number, required: true
#   parameter :b, type: :number, required: true
#
#   execute do |params|
#     a = params[:a]
#     b = params[:b]
#     case params[:operation]
#     when 'add' then a + b
#     when 'subtract' then a - b
#     when 'multiply' then a * b
#     when 'divide' then a / b
#     end
#   end
# end
#
# # Use the tool
# result = calculator.call(operation: 'add', a: 5, b: 3)
# puts result # => 8
#
# # Generate schema for different providers
# puts calculator.to_openai_schema
# puts calculator.to_anthropic_schema
# ```
#
# @see Cosmos::Llm::Tool::Definition For defining tools
# @see Cosmos::Llm::Tool::Registry For managing tool collections

# require 'zeitwerk'
# loader = Zeitwerk::Loader.new
# loader.tag = File.basename(__FILE__, '.rb')
# loader.inflector = Zeitwerk::GemInflector.new(__FILE__)
# loader.push_dir("#{File.dirname(__FILE__)}/../../..")

require 'cosmos/llm/tool/version'

module Cosmos
  module Llm
    # The Tool module provides a comprehensive tool system for LLMs.
    #
    # This module serves as the main entry point for the Cosmos LLM Tool gem, offering:
    # - Tool definition DSL
    # - Parameter validation and type checking
    # - Schema generation for multiple LLM providers
    # - Tool registry and discovery
    # - Execution management and error handling
    #
    # @example Defining and using a tool
    #   tool = Cosmos::Llm::Tool.define(:weather) do
    #     description 'Get current weather for a location'
    #     parameter :location, type: :string, required: true
    #     execute { |params| "Weather in #{params[:location]}: Sunny" }
    #   end
    #
    # @see Cosmos::Llm::Tool::Definition
    # @see Cosmos::Llm::Tool::Registry
    module Tool
      class << self
        # @return [Registry] The global tool registry
        attr_accessor :registry

        # Returns the global tool registry.
        #
        # @return [Registry] The global registry instance
        def global_registry
          @registry ||= Registry.new
        end

        # Defines a new tool and optionally registers it globally.
        #
        # This is a convenience method that creates a new tool definition
        # using the DSL and optionally adds it to the global registry.
        #
        # @param name [Symbol, String] The name of the tool
        # @param register [Boolean] Whether to register the tool globally
        # @yield The block to configure the tool
        # @return [Definition] The tool definition
        # @example Define a simple tool
        #   tool = Cosmos::Llm::Tool.define(:greet) do
        #     description 'Greets a person'
        #     parameter :name, type: :string
        #     execute { |params| "Hello, #{params[:name]}!" }
        #   end
        def define(name, register: true, &block)
          tool = Definition.new(name, &block)
          global_registry.register(tool) if register
          tool
        end

        # Retrieves a tool from the global registry.
        #
        # @param name [Symbol, String] The name of the tool
        # @return [Definition, nil] The tool definition or nil if not found
        # @example Get a registered tool
        #   tool = Cosmos::Llm::Tool.get(:calculator)
        def get(name)
          global_registry.get(name)
        end

        # Lists all registered tools.
        #
        # @return [Array<Definition>] Array of all registered tools
        def all
          global_registry.all
        end

        # Clears all tools from the global registry.
        #
        # @return [void]
        def clear!
          global_registry.clear
        end
      end
    end
  end
end

require 'cosmos/llm/tool/definition'
require 'cosmos/llm/tool/parameter'
require 'cosmos/llm/tool/registry'
require 'cosmos/llm/tool/schemas'
require 'cosmos/llm/tool/executor'
require 'cosmos/llm/tool/errors'

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
