# frozen_string_literal: true

module Cosmos
  module Llm
    module Predict
      module Adapters
        # Translates between a signature and a provider.
        #
        # An adapter has two jobs: turn a signature plus input values into
        # messages the model can answer, and turn the model's reply back into a
        # hash of output field values. Everything provider-specific about
        # prompting strategy lives here, which is what keeps {Signature}
        # declarative and {Module} strategy-agnostic.
        #
        # Subclasses implement {#format_instructions} and {#extract}. The base
        # class handles message assembly, few-shot demonstrations, and type
        # coercion, which are the same regardless of output format.
        #
        # @abstract Subclass and implement {#format_instructions} and {#extract}.
        # @see ChatAdapter
        # @see JsonAdapter
        class Base
          # Builds the messages for one call.
          #
          # @param signature [Class] the signature being satisfied
          # @param inputs [Hash] the input field values
          # @param demos [Array<Example>] few-shot demonstrations
          # @return [Array<Hash>] messages in role/content form
          # @example
          #   adapter.format(Summarize, { document: 'text' }, [])
          def format(signature, inputs, demos = [])
            messages = [{ role: 'system', content: system_prompt(signature) }]
            demos.each do |demo|
              messages << { role: 'user', content: render_inputs(signature, demo_inputs(signature, demo)) }
              messages << { role: 'assistant', content: render_demo_outputs(signature, demo) }
            end
            messages << { role: 'user', content: render_inputs(signature, inputs) }
            messages
          end

          # Parses a model response into coerced output values.
          #
          # @param signature [Class] the signature being satisfied
          # @param completion [String] the raw model output
          # @return [Hash{Symbol => Object}] coerced output values
          # @raise [ParseError] if the output fields cannot be extracted
          def parse(signature, completion)
            raw = extract(signature, completion.to_s)
            signature.coerce_outputs(raw)
          rescue Cosmos::Llm::Signature::Error => e
            raise ParseError.new(e.message, completion: completion, signature: signature)
          end

          # Extra parameters to send with the request.
          #
          # Adapters that use native provider features (structured output, for
          # instance) override this to declare them.
          #
          # @param _signature [Class] the signature being satisfied
          # @return [Hash] parameters merged into the completion request
          def request_params(_signature)
            {}
          end

          # The system prompt: instructions, field descriptions, and the format
          # contract the model is expected to honor.
          #
          # @param signature [Class] the signature being satisfied
          # @return [String] the system prompt
          def system_prompt(signature)
            [
              signature.instructions,
              field_description(signature),
              format_instructions(signature)
            ].reject { |part| part.to_s.strip.empty? }.join("\n\n")
          end

          # Describes the signature's fields in prose for the system prompt.
          #
          # @param signature [Class] the signature being satisfied
          # @return [String] the field description block
          def field_description(signature)
            lines = ['Your input fields are:']
            signature.input_fields.each_value { |field| lines << describe_field(field) }
            lines << ''
            lines << 'Your output fields are:'
            signature.output_fields.each_value { |field| lines << describe_field(field) }
            lines.join("\n")
          end

          # The format contract shown to the model.
          #
          # @abstract
          # @param signature [Class] the signature being satisfied
          # @return [String] instructions describing the expected output format
          # @raise [NotImplementedError] unless overridden
          def format_instructions(signature)
            raise NotImplementedError, "#{self.class} must implement #format_instructions"
          end

          # Pulls raw output field values out of a completion.
          #
          # @abstract
          # @param signature [Class] the signature being satisfied
          # @param completion [String] the raw model output
          # @return [Hash] raw values keyed by field name, before coercion
          # @raise [NotImplementedError] unless overridden
          # @raise [ParseError] if the output cannot be parsed
          def extract(signature, completion)
            raise NotImplementedError, "#{self.class} must implement #extract"
          end

          # Renders the input values for a user message.
          #
          # @abstract
          # @param signature [Class] the signature being satisfied
          # @param inputs [Hash] the input values
          # @return [String] the rendered user message
          # @raise [NotImplementedError] unless overridden
          def render_inputs(signature, inputs)
            raise NotImplementedError, "#{self.class} must implement #render_inputs"
          end

          # Renders a demonstration's outputs as an assistant message.
          #
          # @abstract
          # @param signature [Class] the signature being satisfied
          # @param demo [Example] the demonstration
          # @return [String] the rendered assistant message
          # @raise [NotImplementedError] unless overridden
          def render_demo_outputs(signature, demo)
            raise NotImplementedError, "#{self.class} must implement #render_demo_outputs"
          end

          private

          # @param field [Cosmos::Llm::Signature::Field] the field to describe
          # @return [String] a single description line
          def describe_field(field)
            line = "- #{field.name} (#{field.type_hint})"
            line += ": #{field.desc}" unless field.desc.to_s.empty?
            line
          end

          # Extracts a demonstration's input values for the signature.
          #
          # @param signature [Class] the signature being satisfied
          # @param demo [Example] the demonstration
          # @return [Hash] the demo's values for the signature's input fields
          def demo_inputs(signature, demo)
            values = demo.respond_to?(:to_h) ? demo.to_h : demo
            signature.input_fields.keys.each_with_object({}) do |name, result|
              result[name] = values[name] if values.key?(name)
            end
          end

          # Extracts a demonstration's output values for the signature.
          #
          # @param signature [Class] the signature being satisfied
          # @param demo [Example] the demonstration
          # @return [Hash] the demo's values for the signature's output fields
          def demo_outputs(signature, demo)
            values = demo.respond_to?(:to_h) ? demo.to_h : demo
            signature.output_fields.keys.each_with_object({}) do |name, result|
              result[name] = values[name] if values.key?(name)
            end
          end

          # Renders a value for inclusion in a prompt.
          #
          # @param value [Object] the value
          # @return [String] the rendered value
          def render_value(value)
            case value
            when String then value
            when Array, Hash then JSON.generate(value)
            else value.to_s
            end
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
