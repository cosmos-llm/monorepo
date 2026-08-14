# frozen_string_literal: true

require_relative 'example'

module Cosmos
  module Llm
    module Predict
      # The result of running a module: typed output values plus what produced them.
      #
      # A Prediction is an {Example} carrying the coerced output fields, with the
      # raw completion text, token usage, and the signature attached so callers
      # can inspect or log the call after the fact.
      #
      # @example
      #   prediction = predictor.call(question: 'What is 2+2?')
      #   prediction.answer      # => '4'
      #   prediction[:answer]    # => '4'
      #   prediction.usage       # => { 'input_tokens' => 34, ... }
      class Prediction < Example
        # @return [String] the raw text the model returned
        attr_accessor :completion

        # @return [Hash, nil] provider-reported token usage, if available
        attr_accessor :usage

        # @return [Class, nil] the signature this prediction satisfies
        attr_accessor :signature

        # @return [Boolean] whether the value came from a cache
        attr_accessor :cached

        # Builds a prediction.
        #
        # @param values [Hash] the coerced output values
        def initialize(**values)
          super
          @completion = ''
          @usage = nil
          @signature = nil
          @cached = false
        end

        # @return [Boolean] whether this prediction was served from a cache
        def cached?
          @cached
        end

        # Reads a declared output field, raising for anything undeclared.
        #
        # Unlike {Example#[]}, which returns nil for absent keys, this raises.
        # A typo in a field name is a bug worth surfacing, not a silent nil that
        # propagates into a metric.
        #
        # @param key [Symbol, String] the field name
        # @return [Object] the value
        # @raise [UnknownFieldError] if the field was not produced
        # @example
        #   prediction.fetch(:answer)
        def fetch(key)
          name = key.to_s.to_sym
          return @values[name] if @values.key?(name)

          raise UnknownFieldError,
                "Prediction has no field #{name} (have: #{@values.keys.join(', ')})"
        end

        # @return [String] string representation
        def to_s
          "#<Prediction #{@values.inspect}>"
        end
        alias inspect to_s

        private

        # Carries prediction metadata across copies made by {Example#merge} and
        # friends, so a derived prediction keeps its provenance.
        #
        # @param values [Hash] the values for the copy
        # @return [Prediction] a new prediction
        def dup_with(values)
          copy = self.class.new(**values)
          copy.completion = @completion
          copy.usage = @usage
          copy.signature = @signature
          copy.cached = @cached
          copy
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
