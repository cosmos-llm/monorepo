# frozen_string_literal: true

module Cosmos
  module Llm
    module Tool
      # Represents a tool definition with parameters and execution logic.
      #
      # This class provides a DSL for defining tools that can be used with LLMs,
      # including parameter specifications, descriptions, and execution handlers.
      #
      # @example Define a tool
      #   tool = Definition.new(:calculator) do
      #     description 'A simple calculator'
      #     parameter :operation, type: :string, required: true
      #     parameter :a, type: :number, required: true
      #     parameter :b, type: :number, required: true
      #     execute { |params| params[:a] + params[:b] }
      #   end
      #
      # @see Tool
      class Definition
        # @return [Symbol] The name of the tool
        attr_reader :name

        # @return [String] The description of the tool
        attr_accessor :description

        # @return [Array<Parameter>] The parameters for this tool
        attr_reader :parameters

        # @return [Proc] The execution handler
        attr_accessor :handler

        # Initializes a new tool definition.
        #
        # @param name [Symbol, String] The name of the tool
        # @yield Block to configure the tool using the DSL
        # @return [Definition] A new tool definition
        def initialize(name, &block)
          raise ArgumentError, 'Tool name cannot be nil' if name.nil?

          @name = name.to_s.to_sym
          @description = ''
          @parameters = []
          @handler = nil
          instance_eval(&block) if block_given?
        end

        # Sets the description for the tool.
        #
        # @param text [String] The description text
        # @return [String] The description
        # @example Set description
        #   description 'Calculates the sum of two numbers'
        def description(text = nil)
          @description = text unless text.nil?
          @description
        end

        # Adds a parameter to the tool.
        #
        # @param name [Symbol, String] The parameter name
        # @param options [Hash] Parameter options
        # @option options [Symbol] :type The parameter type (:string, :number, :boolean, :array, :object)
        # @option options [String] :description Parameter description
        # @option options [Boolean] :required Whether the parameter is required
        # @option options [Array] :enum Allowed values for the parameter
        # @option options [Object] :default Default value
        # @return [Parameter] The created parameter
        # @example Add a parameter
        #   parameter :name, type: :string, required: true, description: 'User name'
        def parameter(name, **options)
          param = Parameter.new(name, **options)
          @parameters << param
          param
        end

        # Sets the execution handler for the tool.
        #
        # The block receives a hash of parameter values and should return
        # the result of the tool execution.
        #
        # @yield [params] The execution block
        # @yieldparam params [Hash] The validated parameter values
        # @return [Proc] The handler
        # @example Set execution handler
        #   execute do |params|
        #     "Hello, #{params[:name]}!"
        #   end
        def execute(block = nil, &blk)
          @handler = blk || block if blk || block
          @handler
        end

        # Calls the tool with the given parameters.
        #
        # @param params [Hash] The parameter values
        # @return [Object] The result of the tool execution
        # @raise [ValidationError] If parameter validation fails
        # @raise [ExecutionError] If the tool execution fails
        # @example Call the tool
        #   result = tool.call(name: 'Alice')
        def call(params = {})
          Executor.execute(self, params)
        end

        # Generates an OpenAI function schema for this tool.
        #
        # @return [Hash] The OpenAI function schema
        # @example Generate OpenAI schema
        #   schema = tool.to_openai_schema
        def to_openai_schema
          Schemas::OpenAi.generate(self)
        end

        # Generates an Anthropic tool schema for this tool.
        #
        # @return [Hash] The Anthropic tool schema
        # @example Generate Anthropic schema
        #   schema = tool.to_anthropic_schema
        def to_anthropic_schema
          Schemas::Anthropic.generate(self)
        end

        # Generates a generic JSON schema for this tool.
        #
        # @return [Hash] The JSON schema
        # @example Generate JSON schema
        #   schema = tool.to_json_schema
        def to_json_schema
          Schemas::JsonSchema.generate(self)
        end

        # Converts the tool to a hash representation.
        #
        # @return [Hash] Hash with name, description, parameters, and has_handler
        def to_h
          {
            name: @name,
            description: @description,
            parameters: @parameters.map(&:to_h),
            has_handler: !@handler.nil?
          }
        end

        # Returns a string representation of the tool.
        #
        # @return [String] String representation
        def to_s
          "#<Tool:#{@name} params=#{@parameters.length}>"
        end

        # Inspects the tool.
        #
        # @return [String] Detailed string representation
        def inspect
          "#<Cosmos::Llm::Tool::Definition:0x#{object_id.to_s(16)} @name=#{@name.inspect} @parameters=#{@parameters.inspect}>"
        end

        private

        # Converts a parameter type to JSON schema type.
        #
        # @param type [Symbol] The parameter type
        # @return [String] The JSON schema type
        # @raise [ArgumentError] If type is unknown
        def json_schema_type(type)
          case type
          when :number then 'number'
          when :integer then 'integer'
          when :boolean then 'boolean'
          when :array then 'array'
          when :object then 'object'
          when :string then 'string'
          else
            raise ArgumentError, "Unknown parameter type: #{type}"
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
