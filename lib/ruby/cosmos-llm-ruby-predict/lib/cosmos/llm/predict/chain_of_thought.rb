# frozen_string_literal: true

require_relative 'predictor'

module Cosmos
  module Llm
    module Predict
      # A predictor that asks the model to reason before answering.
      #
      # Derives a new signature with a +reasoning+ output field placed ahead of
      # the declared outputs. Because adapters render output fields in order, the
      # model writes its reasoning first and its answer after, which is the whole
      # mechanism — no prompt engineering beyond field ordering.
      #
      # The reasoning is kept on the prediction, so it is available for logging
      # or for a metric that cares how an answer was reached.
      #
      # @example
      #   cot = ChainOfThought.new('question -> answer')
      #   prediction = cot.call(question: 'If a train leaves at 3pm ...')
      #   prediction.reasoning  # => 'The train travels for two hours, so ...'
      #   prediction.answer     # => '5pm'
      class ChainOfThought < Predict
        # Default description for the injected reasoning field.
        REASONING_DESC = 'Think step by step. Work through the problem before giving the other fields.'

        # Builds a chain-of-thought predictor.
        #
        # @param signature [Class, String] a signature class or shorthand string
        # @param reasoning_field [Symbol] name for the injected field
        # @param reasoning_desc [String] description for the injected field
        # @param options [Hash] remaining options passed to {Predict#initialize}
        # @raise [Cosmos::Llm::Signature::DefinitionError] if the signature already
        #   declares a field with the reasoning field's name
        def initialize(signature, reasoning_field: :reasoning, reasoning_desc: REASONING_DESC, **options)
          base = Cosmos::Llm::Signature.coerce(signature)
          super(self.class.extend_signature(base, reasoning_field, reasoning_desc), **options)
        end

        # Builds a signature with the reasoning field placed before the outputs.
        #
        # Field order is declaration order, and the base signature's outputs are
        # already declared, so the extended signature is rebuilt from scratch
        # rather than appended to.
        #
        # @param base [Class] the original signature
        # @param field_name [Symbol] name for the injected field
        # @param field_desc [String] description for the injected field
        # @return [Class] the extended signature
        # @raise [Cosmos::Llm::Signature::DefinitionError] on a name collision
        def self.extend_signature(base, field_name, field_desc)
          if base.fields.key?(field_name)
            raise Cosmos::Llm::Signature::DefinitionError,
                  "Signature already declares a #{field_name} field; pass reasoning_field: to use another name."
          end

          inputs = base.input_fields
          outputs = base.output_fields
          text = base.instructions

          Cosmos::Llm::Signature.build do
            inputs.each_value do |field|
              input(field.name, field.type, desc: field.desc, of: field.element_type, enum: field.enum,
                                            required: field.required?, default: field.default)
            end
            output(field_name, String, desc: field_desc)
            outputs.each_value do |field|
              output(field.name, field.type, desc: field.desc, of: field.element_type, enum: field.enum,
                                             required: field.required?, default: field.default)
            end
            instructions(text)
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
