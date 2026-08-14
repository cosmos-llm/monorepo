# frozen_string_literal: true

require_relative 'base'

module Cosmos
  module Llm
    module Predict
      module Adapters
        # Delimits fields with section headers, so any chat model can comply.
        #
        # Each field is introduced by a marker line and followed by its value:
        #
        #   [[ ## summary ## ]]
        #   The document argues that ...
        #
        #   [[ ## confidence ## ]]
        #   0.82
        #
        #   [[ ## done ## ]]
        #
        # This asks less of the model than strict JSON: no escaping, no balanced
        # braces, and multi-line prose values need no special handling. That
        # makes it the safer default for smaller or older models, which is why
        # it is the default adapter.
        #
        # @see JsonAdapter for models with reliable structured output
        class ChatAdapter < Base
          # Matches a field header line, capturing the field name.
          HEADER_PATTERN = /^\s*\[\[\s*##\s*(\w+)\s*##\s*\]\]\s*$/.freeze

          # Name of the terminator marker the model emits after the last field.
          TERMINATOR = 'done'

          # (see Base#format_instructions)
          def format_instructions(signature)
            fields = signature.output_fields.values
            lines = [
              'Respond with each output field in its own section, in this exact structure:',
              ''
            ]
            fields.each do |field|
              lines << marker(field.name)
              lines << "<#{field.name}, #{field.type_hint}>"
              lines << ''
            end
            lines << marker(TERMINATOR)
            lines << ''
            lines << 'Emit every field header exactly as shown, including the brackets and hashes. ' \
                     'Put nothing before the first header.'
            lines.join("\n")
          end

          # (see Base#render_inputs)
          def render_inputs(signature, inputs)
            sections = signature.input_fields.keys.filter_map do |name|
              next unless inputs.key?(name)

              "#{marker(name)}\n#{render_value(inputs[name])}"
            end
            sections << marker(TERMINATOR)
            sections.join("\n\n")
          end

          # (see Base#render_demo_outputs)
          def render_demo_outputs(signature, demo)
            values = demo_outputs(signature, demo)
            sections = values.map { |name, value| "#{marker(name)}\n#{render_value(value)}" }
            sections << marker(TERMINATOR)
            sections.join("\n\n")
          end

          # Splits a completion into field sections.
          #
          # Missing sections are simply absent from the result; the signature
          # decides whether that is an error, since it is the thing that knows
          # which fields are required.
          #
          # @param signature [Class] the signature being satisfied
          # @param completion [String] the raw model output
          # @return [Hash{Symbol => String}] raw section text keyed by field name
          # @raise [ParseError] if no field headers appear at all
          def extract(signature, completion)
            sections = split_sections(completion)
            if sections.empty?
              raise ParseError.new(
                "Expected field headers like #{marker(signature.output_fields.keys.first)} " \
                'but found none in the response.',
                completion: completion,
                signature: signature
              )
            end

            declared = signature.output_fields.keys
            sections.select { |name, _| declared.include?(name) }
          end

          private

          # @param name [Symbol, String] the field name
          # @return [String] the header marker for that field
          def marker(name)
            "[[ ## #{name} ## ]]"
          end

          # Walks the completion line by line, accumulating each section body.
          #
          # @param completion [String] the raw model output
          # @return [Hash{Symbol => String}] section text keyed by field name
          def split_sections(completion)
            sections = {}
            current = nil
            buffer = []

            completion.to_s.each_line do |line|
              match = HEADER_PATTERN.match(line)
              if match
                sections[current] = buffer.join.strip if current
                current = match[1].to_sym
                current = nil if current.to_s == TERMINATOR
                buffer = []
              elsif current
                buffer << line
              end
            end
            sections[current] = buffer.join.strip if current

            sections
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
