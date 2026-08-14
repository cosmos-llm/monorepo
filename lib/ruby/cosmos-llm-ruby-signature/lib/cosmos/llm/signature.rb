# frozen_string_literal: true

require 'json'

require_relative 'signature/version'
require_relative 'signature/errors'
require_relative 'signature/field'
require_relative 'signature/parser'

module Cosmos
  module Llm
    # A declarative description of one LLM call: named typed inputs, named typed
    # outputs, and the instructions that connect them.
    #
    # A signature says *what* a call does, not *how* to prompt for it. Rendering
    # the fields into messages and parsing the response back into typed values is
    # the job of an adapter (see the cosmos-llm-predict gem), which means the same
    # signature works across providers and across prompting strategies.
    #
    # There are three ways to declare one. Subclassing is the most explicit:
    #
    # @example Subclass form
    #   class Summarize < Cosmos::Llm::Signature
    #     instructions 'Summarize the document for a technical reader.'
    #     input  :document, String, desc: 'raw text to summarize'
    #     output :summary, String, desc: 'three sentences, no preamble'
    #     output :confidence, Float
    #   end
    #
    #   Summarize.input_fields.keys  # => [:document]
    #
    # @example Block form, for signatures built at runtime
    #   sig = Cosmos::Llm::Signature.build do
    #     input  :question
    #     output :answer
    #   end
    #
    # @example Shorthand string form
    #   sig = Cosmos::Llm::Signature.parse('question -> answer: string')
    #
    # Instances hold no state beyond their class-level declarations; a signature
    # is a description, and the values that flow through it live in Example and
    # Prediction objects instead.
    class Signature
      class << self
        # Declares an input field.
        #
        # @param name [Symbol, String] the field name
        # @param type [Class, Symbol] the field type, defaults to +String+
        # @param options [Hash] additional options passed to {Field}
        # @option options [String] :desc description shown to the model
        # @option options [Class, Symbol] :of element type for array fields
        # @option options [Array] :enum allowed values
        # @option options [Boolean] :required whether the field must be present
        # @option options [Object] :default value used when absent
        # @return [Field] the declared field
        # @raise [DefinitionError] if the name collides with an existing field
        # @example
        #   input :question, String, desc: 'the user question'
        def input(name, type = String, **options)
          declare_field(name, kind: :input, type: type, **options)
        end

        # Declares an output field.
        #
        # @param (see .input)
        # @option (see .input)
        # @return [Field] the declared field
        # @raise [DefinitionError] if the name collides with an existing field
        # @example
        #   output :answer, String, desc: 'a direct answer'
        def output(name, type = String, **options)
          declare_field(name, kind: :output, type: type, **options)
        end

        # Sets or reads the instructions for this signature.
        #
        # With no argument this reads the current value, falling back to a
        # generated default derived from the field names.
        #
        # @param text [String, nil] the instructions to set
        # @return [String] the instructions
        # @example
        #   instructions 'Answer using only the supplied context.'
        def instructions(text = nil)
          @instructions = text unless text.nil?
          @instructions || default_instructions
        end

        # Replaces the instructions and returns a new anonymous signature class.
        #
        # The receiver is left untouched, which is what optimizers need when they
        # try instruction variants against a fixed set of fields.
        #
        # @param text [String] the new instructions
        # @return [Class] a subclass with the new instructions
        # @example
        #   Variant = Summarize.with_instructions('Be terse.')
        def with_instructions(text)
          derive { instructions(text) }
        end

        # Builds a new signature class with additional or replacement fields.
        #
        # @yield the block is evaluated in the new class, so it may call
        #   {.input}, {.output}, and {.instructions}
        # @return [Class] a subclass carrying this signature's fields plus the
        #   block's changes
        # @example Add a field to an existing signature
        #   WithReasoning = Answer.derive { output :reasoning, String }
        def derive(&block)
          child = Class.new(self)
          child.class_eval(&block) if block
          child
        end

        # All declared fields, in declaration order.
        #
        # @return [Hash{Symbol => Field}] fields keyed by name
        def fields
          own = @fields ||= {}
          return own unless superclass.respond_to?(:fields)

          superclass.fields.merge(own)
        end

        # @return [Hash{Symbol => Field}] input fields keyed by name
        def input_fields
          fields.select { |_name, field| field.input? }
        end

        # @return [Hash{Symbol => Field}] output fields keyed by name
        def output_fields
          fields.select { |_name, field| field.output? }
        end

        # Builds a signature class from a block.
        #
        # @yield evaluated in the new class
        # @return [Class] the new signature class
        # @example
        #   sig = Cosmos::Llm::Signature.build do
        #     input :question
        #     output :answer
        #   end
        def build(&block)
          raise DefinitionError, 'Signature.build requires a block' unless block

          Class.new(Signature) { class_eval(&block) }
        end

        # Builds a signature class from the shorthand string form.
        #
        # @param text [String] e.g. "question -> answer: string"
        # @param instructions [String, nil] optional instructions
        # @return [Class] the new signature class
        # @raise [DefinitionError] if the string is malformed
        # @example
        #   sig = Cosmos::Llm::Signature.parse('document -> summary, score: number')
        def parse(text, instructions: nil)
          inputs, outputs = Parser.parse(text)
          Class.new(Signature) do
            inputs.each { |descriptor| input(descriptor.delete(:name), descriptor.delete(:type) || String, **descriptor) }
            outputs.each do |descriptor|
              output(descriptor.delete(:name), descriptor.delete(:type) || String, **descriptor)
            end
            instructions(instructions) if instructions
          end
        end

        # Accepts a signature class or a shorthand string and returns a class.
        #
        # Useful at API boundaries where callers may pass either form.
        #
        # @param signature [Class, String] a signature class or shorthand
        # @return [Class] a signature class
        # @raise [DefinitionError] if the argument is neither
        # @example
        #   Cosmos::Llm::Signature.coerce('question -> answer')
        def coerce(signature)
          return signature if signature.is_a?(Class) && signature <= Signature
          return parse(signature) if signature.is_a?(String)

          raise DefinitionError,
                "Expected a Signature subclass or a shorthand string, got #{signature.inspect}"
        end

        # Builds a JSON schema describing the output fields.
        #
        # This is what gets handed to providers that support structured output,
        # and what the JSON adapter validates against.
        #
        # @return [Hash] a JSON schema object
        # @example
        #   Summarize.output_json_schema
        #   # => { 'type' => 'object', 'properties' => { ... }, 'required' => [...] }
        def output_json_schema
          properties = {}
          required = []
          output_fields.each do |name, field|
            properties[name.to_s] = field.to_json_schema
            required << name.to_s if field.required?
          end

          {
            'type' => 'object',
            'properties' => properties,
            'required' => required,
            'additionalProperties' => false
          }
        end

        # Coerces a hash of raw output values into declared types.
        #
        # Keys may be strings or symbols. Missing required fields without a
        # default raise; missing optional fields are omitted.
        #
        # @param values [Hash] raw values, typically parsed from a model response
        # @return [Hash{Symbol => Object}] coerced values keyed by field name
        # @raise [MissingFieldError] if a required field is absent
        # @raise [CoercionError] if a value cannot be coerced
        # @example
        #   Summarize.coerce_outputs('summary' => 'text', 'confidence' => '0.9')
        #   # => { summary: 'text', confidence: 0.9 }
        def coerce_outputs(values)
          coerce_fields(output_fields, values)
        end

        # Coerces a hash of raw input values into declared types.
        #
        # @param (see .coerce_outputs)
        # @return (see .coerce_outputs)
        # @raise (see .coerce_outputs)
        def coerce_inputs(values)
          coerce_fields(input_fields, values)
        end

        # A stable identifier for this signature, used for cache keys.
        #
        # Two signatures with the same fields and instructions produce the same
        # value, which is what lets a cache survive process restarts and
        # anonymous classes.
        #
        # @return [String] a stable digest-friendly description
        def cache_signature
          JSON.generate(
            instructions: instructions,
            fields: fields.values.map(&:to_h)
          )
        end

        # @return [String] a readable description of the signature
        def to_s
          ins = input_fields.keys.join(', ')
          outs = output_fields.keys.join(', ')
          "#{name || '#<Signature>'}(#{ins} -> #{outs})"
        end

        private

        # Declares a field and stores it on this class.
        #
        # @param name [Symbol, String] the field name
        # @param kind [Symbol] +:input+ or +:output+
        # @param type [Class, Symbol] the field type
        # @param options [Hash] additional {Field} options
        # @return [Field] the declared field
        # @raise [DefinitionError] on a duplicate name
        def declare_field(name, kind:, type:, **options)
          @fields ||= {}
          key = name.to_s.to_sym
          if @fields.key?(key)
            raise DefinitionError,
                  "Field #{key} is already declared on #{self.name || 'this signature'}"
          end

          @fields[key] = Field.new(key, kind: kind, type: type, **options)
        end

        # Coerces values against a set of fields.
        #
        # @param field_set [Hash{Symbol => Field}] the fields to coerce against
        # @param values [Hash] raw values
        # @return [Hash{Symbol => Object}] coerced values
        # @raise [MissingFieldError] if a required field is absent
        def coerce_fields(field_set, values)
          normalized = normalize_keys(values)
          field_set.each_with_object({}) do |(name, field), result|
            if normalized.key?(name)
              result[name] = field.coerce(normalized[name])
            elsif !field.default.nil?
              result[name] = field.default
            elsif field.required?
              raise MissingFieldError,
                    "Missing required #{field.kind} field #{name} (have: #{normalized.keys.join(', ')})"
            end
          end
        end

        # @param values [Hash] a hash with string or symbol keys
        # @return [Hash{Symbol => Object}] the same hash with symbol keys
        def normalize_keys(values)
          raise DefinitionError, "Expected a Hash of values, got #{values.inspect}" unless values.is_a?(Hash)

          values.each_with_object({}) { |(key, value), result| result[key.to_s.to_sym] = value }
        end

        # @return [String] instructions generated from the field names
        def default_instructions
          ins = input_fields.keys.map { |name| "`#{name}`" }.join(', ')
          outs = output_fields.keys.map { |name| "`#{name}`" }.join(', ')
          "Given the fields #{ins}, produce the fields #{outs}."
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
