# frozen_string_literal: true

module Cosmos
  module Llm
    module Predict
      # Base error class for all prediction errors.
      class Error < StandardError; end

      # Raised when an adapter cannot extract the signature's output fields from
      # a model response.
      #
      # Carries the raw completion text so callers can log what actually came
      # back, which is usually the fastest way to see why parsing failed.
      class ParseError < Error
        # @return [String] the raw model output that could not be parsed
        attr_reader :completion

        # @return [Class, nil] the signature being parsed against
        attr_reader :signature

        # @param message [String] the error message
        # @param completion [String] the raw model output
        # @param signature [Class, nil] the signature being parsed against
        def initialize(message, completion: '', signature: nil)
          @completion = completion.to_s
          @signature = signature
          super(message)
        end
      end

      # Raised when a prediction is asked for a field its signature does not
      # declare.
      class UnknownFieldError < Error; end

      # Raised when a module is constructed with an invalid configuration.
      class ConfigurationError < Error; end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
