# frozen_string_literal: true

module Cosmos
  module Llm
    module Tool
      # Handles execution of tools with parameter validation and error handling.
      #
      # This class manages the execution lifecycle of tools, including parameter
      # validation, default value application, and error handling.
      #
      # @see Definition
      module Executor
        # Executes a tool with the given parameters.
        #
        # @param tool [Definition] The tool to execute
        # @param params [Hash] The parameter values
        # @return [Object] The result of the tool execution
        # @raise [NoHandlerError] If the tool has no execution handler
        # @raise [ValidationError] If parameter validation fails
        # @raise [ExecutionError] If the tool execution fails
        # @example Execute a tool
        #   result = Executor.execute(tool, name: 'Alice')
        def self.execute(tool, params = {})
          raise NoHandlerError, "Tool '#{tool.name}' has no execution handler" unless tool.handler

          # Normalize parameter keys to symbols
          normalized_params = normalize_params(params || {})

          # Apply defaults and validate
          validated_params = validate_and_apply_defaults(tool, normalized_params)

          # Execute the tool
          begin
            tool.handler.call(validated_params)
          rescue StandardError => e
            raise ExecutionError, "Error executing tool '#{tool.name}': #{e.message}"
          end
        end

        # Normalizes parameter keys to symbols.
        #
        # @param params [Hash] The parameters to normalize
        # @return [Hash] Normalized parameters
        def self.normalize_params(params)
          (params || {}).transform_keys(&:to_sym)
        end

        # Validates parameters and applies default values.
        #
        # @param tool [Definition] The tool definition
        # @param params [Hash] The parameter values
        # @return [Hash] Validated parameters with defaults applied
        # @raise [ValidationError] If validation fails
        def self.validate_and_apply_defaults(tool, params)
          result = {}

          tool.parameters.each do |param|
            value = params[param.name]

            # Apply default if no value provided
            value = param.default if value.nil? && !param.default.nil?

            # Validate the parameter
            param.validate(value)

            # Only include non-nil values
            result[param.name] = value unless value.nil?
          end

          result
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
