# frozen_string_literal: true

module Cosmos
  module Llm
    class Signature
      # Parses the shorthand signature string form into field declarations.
      #
      # The shorthand is a compact way to declare a signature without opening a
      # block: input names, an arrow, then output names. Each name may carry an
      # optional type annotation after a colon.
      #
      #   "question -> answer"
      #   "document, style -> summary: string, confidence: number"
      #   "text -> tags: array[string]"
      #
      # @see Signature.parse
      module Parser
        # Splits inputs from outputs.
        ARROW = '->'

        # Matches +name+, +name: type+, or +name: array[element]+.
        FIELD_PATTERN = /\A(?<name>\w+)(?:\s*:\s*(?<type>\w+)(?:\s*\[\s*(?<of>\w+)\s*\])?)?\z/.freeze

        module_function

        # Parses a shorthand signature string into field descriptors.
        #
        # @param text [String] the shorthand, e.g. "question -> answer"
        # @return [Array<Array<Hash>>] a two-element array: input descriptors
        #   and output descriptors, each a hash of keyword arguments for {Field}
        # @raise [DefinitionError] if the string is malformed
        # @example
        #   Parser.parse('question -> answer: string')
        #   # => [[{ name: :question, type: :string }], [{ name: :answer, type: :string }]]
        def parse(text)
          raise DefinitionError, 'Signature string cannot be nil' if text.nil?

          left, right, *extra = text.to_s.split(ARROW)
          unless right && extra.empty?
            raise DefinitionError,
                  "Signature string must contain exactly one '#{ARROW}', got #{text.inspect}"
          end

          inputs = parse_side(left, 'input')
          outputs = parse_side(right, 'output')
          raise DefinitionError, "Signature string #{text.inspect} declares no input fields" if inputs.empty?
          raise DefinitionError, "Signature string #{text.inspect} declares no output fields" if outputs.empty?

          [inputs, outputs]
        end

        # Parses one side of the arrow into field descriptors.
        #
        # @param text [String] the comma-separated field list
        # @param side [String] "input" or "output", used in error messages
        # @return [Array<Hash>] field descriptors
        # @raise [DefinitionError] if a field declaration is malformed
        def parse_side(text, side)
          text.to_s.split(',').map(&:strip).reject(&:empty?).map do |declaration|
            match = FIELD_PATTERN.match(declaration)
            unless match
              raise DefinitionError,
                    "Malformed #{side} field #{declaration.inspect}. Expected 'name', 'name: type', " \
                    "or 'name: array[type]'."
            end

            descriptor = { name: match[:name].to_sym }
            descriptor[:type] = match[:type].to_sym if match[:type]
            descriptor[:of] = match[:of].to_sym if match[:of]
            descriptor
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
