# frozen_string_literal: true

require 'json'

module Cosmos
  module Llm
    class Signature
      # A single declared field on a signature, either an input or an output.
      #
      # A field carries a name, a type, an optional human description, and
      # optional constraints. The type drives three things: the JSON schema
      # emitted for structured output, the type hint rendered into the prompt,
      # and the coercion applied to whatever the model returns.
      #
      # Types are given as Ruby classes (+String+, +Integer+, +Float+,
      # +TrueClass+/+FalseClass+ via +:boolean+, +Array+, +Hash+) or as symbols
      # (+:string+, +:integer+, +:number+, +:boolean+, +:array+, +:object+).
      # An array type may declare its element type with +of:+.
      #
      # @example A required string input
      #   Field.new(:question, kind: :input, type: String, desc: 'user question')
      #
      # @example A typed array output
      #   Field.new(:tags, kind: :output, type: Array, of: String)
      #
      # @see Signature
      class Field
        # Maps Ruby classes and shorthand symbols onto canonical type symbols.
        TYPE_ALIASES = {
          String => :string,
          Integer => :integer,
          Float => :number,
          Numeric => :number,
          TrueClass => :boolean,
          FalseClass => :boolean,
          Array => :array,
          Hash => :object,
          :string => :string,
          :str => :string,
          :integer => :integer,
          :int => :integer,
          :number => :number,
          :float => :number,
          :boolean => :boolean,
          :bool => :boolean,
          :array => :array,
          :list => :array,
          :object => :object,
          :hash => :object
        }.freeze

        # Canonical type symbols this class understands.
        TYPES = %i[string integer number boolean array object].freeze

        # JSON schema type names, keyed by canonical type symbol.
        JSON_SCHEMA_TYPES = {
          string: 'string',
          integer: 'integer',
          number: 'number',
          boolean: 'boolean',
          array: 'array',
          object: 'object'
        }.freeze

        # Human-readable type hints rendered into prompts.
        TYPE_HINTS = {
          string: 'a string',
          integer: 'an integer',
          number: 'a number',
          boolean: 'true or false',
          array: 'a JSON array',
          object: 'a JSON object'
        }.freeze

        # Strings the model may return for a boolean field, normalized.
        TRUTHY = %w[true yes y 1].freeze
        FALSEY = %w[false no n 0].freeze

        # @return [Symbol] the field name
        attr_reader :name

        # @return [Symbol] +:input+ or +:output+
        attr_reader :kind

        # @return [Symbol] the canonical type symbol
        attr_reader :type

        # @return [Symbol, nil] element type for array fields
        attr_reader :element_type

        # @return [String] human description shown to the model
        attr_reader :desc

        # @return [Array, nil] the allowed values, if constrained
        attr_reader :enum

        # @return [Object, nil] default used when the value is absent
        attr_reader :default

        # Builds a field.
        #
        # @param name [Symbol, String] the field name
        # @param kind [Symbol] +:input+ or +:output+
        # @param type [Class, Symbol] the field type
        # @param desc [String, nil] description shown to the model
        # @param of [Class, Symbol, nil] element type for array fields
        # @param enum [Array, nil] allowed values
        # @param required [Boolean] whether the field must be present
        # @param default [Object, nil] value used when absent
        # @raise [DefinitionError] if the name or type is invalid
        def initialize(name, kind:, type: String, desc: nil, of: nil, enum: nil, required: true, default: nil)
          raise DefinitionError, 'Field name cannot be nil' if name.nil?
          raise DefinitionError, "Field kind must be :input or :output, got #{kind.inspect}" unless %i[input
                                                                                                      output].include?(kind)

          @name = name.to_s.to_sym
          @kind = kind
          @type = normalize_type(type)
          @element_type = of.nil? ? nil : normalize_type(of)
          @desc = desc.to_s
          @enum = enum&.map { |value| value.is_a?(Symbol) ? value.to_s : value }
          @required = required
          @default = default
        end

        # @return [Boolean] whether the field must be present
        def required?
          @required
        end

        # @return [Boolean] whether this is an input field
        def input?
          @kind == :input
        end

        # @return [Boolean] whether this is an output field
        def output?
          @kind == :output
        end

        # Renders the type as a short hint for inclusion in a prompt.
        #
        # @return [String] e.g. "a JSON array of strings"
        # @example
        #   field.type_hint # => "a string"
        def type_hint
          hint = if @type == :array && @element_type
                   "a JSON array of #{pluralize_hint(@element_type)}"
                 else
                   TYPE_HINTS.fetch(@type)
                 end
          return "#{hint}, one of: #{@enum.map(&:to_s).join(', ')}" if @enum

          hint
        end

        # Builds the JSON schema fragment describing this field.
        #
        # @return [Hash] a JSON schema property definition
        # @example
        #   field.to_json_schema # => { 'type' => 'string', 'description' => '...' }
        def to_json_schema
          schema = { 'type' => JSON_SCHEMA_TYPES.fetch(@type) }
          schema['description'] = @desc unless @desc.empty?
          schema['enum'] = @enum if @enum
          if @type == :array
            schema['items'] = @element_type ? { 'type' => JSON_SCHEMA_TYPES.fetch(@element_type) } : {}
          end
          schema
        end

        # Coerces a raw value from the model into this field's type.
        #
        # Strings are parsed where the target type needs it: numbers via
        # +Integer()+/+Float()+, booleans against {TRUTHY} and {FALSEY}, arrays
        # and objects via JSON. A value already of the right type passes through.
        #
        # @param value [Object] the raw value
        # @return [Object] the coerced value
        # @raise [CoercionError] if the value cannot be coerced
        # @example
        #   field.coerce('42') # => 42, for an :integer field
        def coerce(value)
          return @default if value.nil?

          coerced = case @type
                    when :string then coerce_string(value)
                    when :integer then coerce_integer(value)
                    when :number then coerce_number(value)
                    when :boolean then coerce_boolean(value)
                    when :array then coerce_array(value)
                    when :object then coerce_object(value)
                    end
          validate_enum!(coerced)
          coerced
        end

        # @return [Hash] hash representation of the field
        def to_h
          {
            name: @name,
            kind: @kind,
            type: @type,
            element_type: @element_type,
            desc: @desc,
            enum: @enum,
            required: @required,
            default: @default
          }
        end

        # @return [String] string representation
        def to_s
          "#<Field:#{@name} #{@kind} #{@type}>"
        end

        private

        # Resolves a class or symbol to a canonical type symbol.
        #
        # @param type [Class, Symbol] the declared type
        # @return [Symbol] canonical type symbol
        # @raise [DefinitionError] if the type is not recognized
        def normalize_type(type)
          resolved = TYPE_ALIASES[type]
          return resolved if resolved

          raise DefinitionError,
                "Unknown field type #{type.inspect}. Expected one of: #{TYPES.join(', ')} " \
                '(or String, Integer, Float, Array, Hash).'
        end

        # @param type [Symbol] a canonical type symbol
        # @return [String] plural hint word for array element descriptions
        def pluralize_hint(type)
          case type
          when :string then 'strings'
          when :integer then 'integers'
          when :number then 'numbers'
          when :boolean then 'booleans'
          when :array then 'arrays'
          when :object then 'objects'
          end
        end

        # @param value [Object] raw value
        # @return [String] the value as a string
        def coerce_string(value)
          return value if value.is_a?(String)
          return JSON.generate(value) if value.is_a?(Array) || value.is_a?(Hash)

          value.to_s
        end

        # @param value [Object] raw value
        # @return [Integer] the value as an integer
        # @raise [CoercionError] if not parseable
        def coerce_integer(value)
          return value if value.is_a?(Integer)
          return value.to_i if value.is_a?(Float) && value.finite?

          Integer(value.to_s.strip, 10)
        rescue ArgumentError, TypeError
          raise CoercionError, "Field #{@name} expected an integer, got #{value.inspect}"
        end

        # @param value [Object] raw value
        # @return [Float] the value as a float
        # @raise [CoercionError] if not parseable
        def coerce_number(value)
          return value.to_f if value.is_a?(Numeric)

          Float(value.to_s.strip)
        rescue ArgumentError, TypeError
          raise CoercionError, "Field #{@name} expected a number, got #{value.inspect}"
        end

        # @param value [Object] raw value
        # @return [Boolean] the value as a boolean
        # @raise [CoercionError] if not recognizable as a boolean
        def coerce_boolean(value)
          return value if value == true || value == false

          normalized = value.to_s.strip.downcase
          return true if TRUTHY.include?(normalized)
          return false if FALSEY.include?(normalized)

          raise CoercionError, "Field #{@name} expected a boolean, got #{value.inspect}"
        end

        # @param value [Object] raw value
        # @return [Array] the value as an array, elements coerced if typed
        # @raise [CoercionError] if not parseable as an array
        def coerce_array(value)
          array = if value.is_a?(Array)
                    value
                  else
                    parsed = parse_json(value)
                    raise CoercionError, "Field #{@name} expected an array, got #{value.inspect}" unless parsed.is_a?(Array)

                    parsed
                  end
          return array unless @element_type

          element_field = self.class.new(:"#{@name}_element", kind: @kind, type: @element_type)
          array.map { |element| element_field.coerce(element) }
        end

        # @param value [Object] raw value
        # @return [Hash] the value as a hash
        # @raise [CoercionError] if not parseable as an object
        def coerce_object(value)
          return value if value.is_a?(Hash)

          parsed = parse_json(value)
          raise CoercionError, "Field #{@name} expected an object, got #{value.inspect}" unless parsed.is_a?(Hash)

          parsed
        end

        # @param value [Object] raw value expected to hold JSON
        # @return [Object] the parsed structure
        # @raise [CoercionError] if the text is not valid JSON
        def parse_json(value)
          JSON.parse(value.to_s)
        rescue JSON::ParserError => e
          raise CoercionError, "Field #{@name} expected JSON, got #{value.inspect} (#{e.message})"
        end

        # @param value [Object] the coerced value
        # @raise [CoercionError] if an enum is declared and the value is outside it
        def validate_enum!(value)
          return if @enum.nil?
          return if @enum.include?(value)
          return if value.is_a?(String) && @enum.map(&:to_s).include?(value)

          raise CoercionError,
                "Field #{@name} expected one of #{@enum.inspect}, got #{value.inspect}"
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
