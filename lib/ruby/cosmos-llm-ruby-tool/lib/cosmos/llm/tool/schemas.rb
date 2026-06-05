# frozen_string_literal: true

module Cosmos
  module Llm
    module Tool
      # Namespace for schema generators.
      #
      # This module contains schema generators for different LLM providers,
      # converting tool definitions into provider-specific formats.
      module Schemas
        # OpenAI function schema generator.
        #
        # Generates schemas compatible with OpenAI's function calling API.
        module OpenAi
          # Generates an OpenAI function schema from a tool definition.
          #
          # @param tool [Definition] The tool to generate schema for
          # @return [Hash] The OpenAI function schema
          def self.generate(tool)
            {
              type: 'function',
              function: {
                name: tool.name.to_s,
                description: tool.description,
                parameters: generate_parameters(tool)
              }
            }
          end

          # Generates the parameters section of the schema.
          #
          # @param tool [Definition] The tool definition
          # @return [Hash] The parameters schema
          def self.generate_parameters(tool)
            properties = {}
            required = []

            tool.parameters.each do |param|
              properties[param.name.to_s] = parameter_schema(param)
              required << param.name.to_s if param.required?
            end

            result = {
              type: 'object',
              properties: properties
            }

            result[:required] = required unless required.empty?
            result
          end

          # Generates schema for a single parameter.
          #
          # @param param [Parameter] The parameter
          # @return [Hash] The parameter schema
          def self.parameter_schema(param)
            schema = {
              type: json_schema_type(param.type)
            }

            schema[:description] = param.description unless param.description.empty?
            schema[:enum] = param.enum if param.enum?
            schema[:default] = param.default unless param.default.nil?

            schema
          end

          # Converts a parameter type to JSON schema type.
          #
          # @param type [Symbol] The parameter type
          # @return [String] The JSON schema type
          # @raise [ArgumentError] If type is unknown
          def self.json_schema_type(type)
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

        # Anthropic tool schema generator.
        #
        # Generates schemas compatible with Anthropic Claude's tool use API.
        module Anthropic
          # Generates an Anthropic tool schema from a tool definition.
          #
          # @param tool [Definition] The tool to generate schema for
          # @return [Hash] The Anthropic tool schema
          def self.generate(tool)
            {
              name: tool.name.to_s,
              description: tool.description,
              input_schema: generate_input_schema(tool)
            }
          end

          # Generates the input schema section.
          #
          # @param tool [Definition] The tool definition
          # @return [Hash] The input schema
          def self.generate_input_schema(tool)
            properties = {}
            required = []

            tool.parameters.each do |param|
              properties[param.name.to_s] = parameter_schema(param)
              required << param.name.to_s if param.required?
            end

            result = {
              type: 'object',
              properties: properties
            }

            result[:required] = required unless required.empty?
            result
          end

          # Generates schema for a single parameter.
          #
          # @param param [Parameter] The parameter
          # @return [Hash] The parameter schema
          def self.parameter_schema(param)
            schema = {
              type: json_schema_type(param.type)
            }

            schema[:description] = param.description unless param.description.empty?
            schema[:enum] = param.enum if param.enum?

            schema
          end

          # Converts a parameter type to JSON schema type.
          #
          # @param type [Symbol] The parameter type
          # @return [String] The JSON schema type
          # @raise [ArgumentError] If type is unknown
          def self.json_schema_type(type)
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

        # Generic JSON Schema generator.
        #
        # Generates standard JSON Schema for tools.
        module JsonSchema
          # Generates a JSON Schema from a tool definition.
          #
          # @param tool [Definition] The tool to generate schema for
          # @return [Hash] The JSON Schema
          def self.generate(tool)
            properties = {}
            required = []

            tool.parameters.each do |param|
              properties[param.name.to_s] = parameter_schema(param)
              required << param.name.to_s if param.required?
            end

            result = {
              :'$schema' => 'http://json-schema.org/draft-07/schema#',
              '$schema' => 'http://json-schema.org/draft-07/schema#',
              type: 'object',
              title: tool.name.to_s,
              description: tool.description,
              properties: properties
            }

            result[:required] = required unless required.empty?
            result
          end

          # Generates schema for a single parameter.
          #
          # @param param [Parameter] The parameter
          # @return [Hash] The parameter schema
          def self.parameter_schema(param)
            schema = {
              type: json_schema_type(param.type)
            }

            schema[:description] = param.description unless param.description.empty?
            schema[:enum] = param.enum if param.enum?
            schema[:default] = param.default unless param.default.nil?

            # Add properties for object types
            if param.type == :object && !param.properties.empty?
              schema[:properties] = param.properties.transform_keys(&:to_s)
            end

            schema
          end

          # Converts a parameter type to JSON schema type.
          #
          # @param type [Symbol] The parameter type
          # @return [String] The JSON schema type
          # @raise [ArgumentError] If type is unknown
          def self.json_schema_type(type)
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
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
