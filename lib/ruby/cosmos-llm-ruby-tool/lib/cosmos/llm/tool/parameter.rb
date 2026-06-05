# frozen_string_literal: true

module Cosmos
  module Llm
    module Tool
      # Represents a parameter definition for a tool.
      #
      # This class encapsulates parameter metadata including type, description,
      # validation rules, and default values.
      #
      # @example Create a parameter
      #   param = Parameter.new(:name, type: :string, required: true)
      #
      # @see Definition
      class Parameter
        # @return [Symbol] The name of the parameter
        attr_reader :name

        # @return [Symbol] The type of the parameter
        attr_reader :type

        # @return [String] The description of the parameter
        attr_accessor :description

        # @return [Boolean] Whether the parameter is required
        attr_reader :required

        # @return [Array, nil] Allowed values for the parameter
        attr_reader :enum

        # @return [Object, nil] Default value for the parameter
        attr_reader :default

        # @return [Hash] Additional properties for object/array types
        attr_reader :properties

        # Valid parameter types
        VALID_TYPES = %i[string number integer boolean array object].freeze

        # Initializes a new Parameter.
        #
        # @param name [Symbol, String] The parameter name
        # @param type [Symbol] The parameter type
        # @param description [String] Parameter description
        # @param required [Boolean] Whether the parameter is required
        # @param enum [Array] Allowed values
        # @param default [Object] Default value
        # @param properties [Hash] Additional properties for object types
        # @return [Parameter] A new parameter instance
        # @raise [ArgumentError] If the type is invalid
        def initialize(name, type: :string, description: '', required: false, enum: nil, default: nil, properties: nil)
          raise ArgumentError, 'Parameter name cannot be nil' if name.nil?

          @name = name.to_s.to_sym
          @type = type.to_sym
          @description = description
          @required = required
          @enum = enum
          @default = default
          @properties = properties || {}

          validate_inputs!
          validate_type!
        end

        # Checks if the parameter is required.
        #
        # @return [Boolean] True if required
        def required?
          @required
        end

        # Checks if the parameter has enum values.
        #
        # @return [Boolean] True if enum is defined
        def enum?
          !@enum.nil? && !@enum.empty?
        end

        # Validates a value against this parameter's specification.
        #
        # @param value [Object] The value to validate
        # @return [Boolean] True if valid
        # @raise [ValidationError] If validation fails
        def validate(value)
          # Check required
          raise ValidationError, "Parameter '#{@name}' is required" if required? && value.nil?

          return true if value.nil? # Optional parameter not provided

          # Check type
          unless valid_type?(value)
            raise ValidationError, "Parameter '#{@name}' must be of type #{@type}, got #{value.class}"
          end

          # Check enum
          if enum? && !@enum.include?(value)
            raise ValidationError, "Parameter '#{@name}' must be one of #{@enum.inspect}, got #{value.inspect}"
          end

          true
        end

        # Converts the parameter to a hash representation.
        #
        # @return [Hash] Hash with parameter metadata
        def to_h
          {
            name: @name,
            type: @type,
            description: @description,
            required: @required,
            enum: @enum,
            default: @default,
            properties: @properties
          }.compact
        end

        private

        # Validates the input parameters.
        #
        # @raise [ArgumentError] If inputs are invalid
        # @return [void]
        def validate_inputs!
          raise ArgumentError, 'enum must be an Array or nil' if @enum && !@enum.is_a?(Array)
          raise ArgumentError, 'properties must be a Hash or nil' if @properties && !@properties.is_a?(Hash)
        end

        # Validates that the type is valid.
        #
        # @raise [ArgumentError] If type is invalid
        # @return [void]
        def validate_type!
          return if VALID_TYPES.include?(@type)

          raise ArgumentError, "Invalid parameter type: #{@type}. Must be one of #{VALID_TYPES.inspect}"
        end

        # Checks if a value matches the parameter type.
        #
        # @param value [Object] The value to check
        # @return [Boolean] True if the value matches the type
        def valid_type?(value)
          case @type
          when :string
            value.is_a?(String)
          when :number
            value.is_a?(Numeric)
          when :integer
            value.is_a?(Integer)
          when :boolean
            value.is_a?(TrueClass) || value.is_a?(FalseClass)
          when :array
            value.is_a?(Array)
          when :object
            value.is_a?(Hash)
          else
            false
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
