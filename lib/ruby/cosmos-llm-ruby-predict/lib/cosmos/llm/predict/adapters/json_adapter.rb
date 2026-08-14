# frozen_string_literal: true

require 'json'

require_relative 'base'

module Cosmos
  module Llm
    module Predict
      module Adapters
        # Asks for a single JSON object holding every output field.
        #
        # This is the tighter contract of the two adapters: one object, keys
        # matching field names, values already in the right JSON type. Where a
        # provider supports native structured output, {#request_params} hands it
        # the signature's schema so the format is enforced rather than requested.
        #
        # Models often wrap JSON in prose or a fenced code block anyway, so
        # extraction tolerates both.
        #
        # @see ChatAdapter for models that struggle with strict JSON
        class JsonAdapter < Base
          # Matches a fenced code block, optionally tagged as json.
          FENCE_PATTERN = /```(?:json)?\s*\n(.*?)```/m.freeze

          # Builds a JSON adapter.
          #
          # @param native_structured_output [Boolean] whether to send the
          #   signature's JSON schema as a +response_format+ request parameter.
          #   Providers that ignore the parameter still work, since the format is
          #   also described in the prompt.
          # @param schema_name [String] name given to the schema in the request
          def initialize(native_structured_output: false, schema_name: 'signature_output')
            @native_structured_output = native_structured_output
            @schema_name = schema_name
          end

          # @return [Boolean] whether native structured output is requested
          def native_structured_output?
            @native_structured_output
          end

          # (see Base#request_params)
          def request_params(signature)
            return {} unless @native_structured_output

            {
              response_format: {
                type: 'json_schema',
                json_schema: {
                  name: @schema_name,
                  strict: true,
                  schema: signature.output_json_schema
                }
              }
            }
          end

          # (see Base#format_instructions)
          def format_instructions(signature)
            [
              'Respond with a single JSON object and nothing else. It must have exactly these keys:',
              '',
              signature.output_fields.values.map { |field| "- \"#{field.name}\": #{field.type_hint}" }.join("\n"),
              '',
              'Do not wrap the object in prose. Do not add keys that are not listed.'
            ].join("\n")
          end

          # (see Base#render_inputs)
          def render_inputs(signature, inputs)
            present = signature.input_fields.keys.each_with_object({}) do |name, result|
              result[name.to_s] = inputs[name] if inputs.key?(name)
            end
            JSON.pretty_generate(present)
          end

          # (see Base#render_demo_outputs)
          def render_demo_outputs(signature, demo)
            values = demo_outputs(signature, demo)
            JSON.generate(values.transform_keys(&:to_s))
          end

          # Pulls the JSON object out of a completion.
          #
          # Tries the whole string first, then a fenced block, then the widest
          # brace-delimited span. Keys not declared on the signature are dropped.
          #
          # @param signature [Class] the signature being satisfied
          # @param completion [String] the raw model output
          # @return [Hash{Symbol => Object}] raw values keyed by field name
          # @raise [ParseError] if no JSON object can be found
          def extract(signature, completion)
            object = parse_object(completion)
            if object.nil?
              raise ParseError.new(
                'Expected a JSON object in the response but could not find one.',
                completion: completion,
                signature: signature
              )
            end

            declared = signature.output_fields.keys
            object.each_with_object({}) do |(key, value), result|
              name = key.to_s.to_sym
              result[name] = value if declared.include?(name)
            end
          end

          private

          # @param completion [String] the raw model output
          # @return [Hash, nil] the parsed object, or nil if none was found
          def parse_object(completion)
            text = completion.to_s
            candidates = [text]
            fenced = FENCE_PATTERN.match(text)
            candidates << fenced[1] if fenced
            braced = balanced_span(text)
            candidates << braced if braced

            candidates.each do |candidate|
              parsed = try_parse(candidate)
              return parsed if parsed.is_a?(Hash)
            end
            nil
          end

          # @param text [String] candidate JSON text
          # @return [Object, nil] the parsed value, or nil if invalid
          def try_parse(text)
            JSON.parse(text.to_s.strip)
          rescue JSON::ParserError
            nil
          end

          # Finds the widest brace-delimited span in the text.
          #
          # Scans for the first opening brace and the last closing brace rather
          # than counting depth, which is enough for the common case of an object
          # surrounded by prose and cheaper than a real parser.
          #
          # @param text [String] the raw model output
          # @return [String, nil] the span, or nil if braces are absent
          def balanced_span(text)
            first = text.index('{')
            last = text.rindex('}')
            return nil if first.nil? || last.nil? || last <= first

            text[first..last]
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
