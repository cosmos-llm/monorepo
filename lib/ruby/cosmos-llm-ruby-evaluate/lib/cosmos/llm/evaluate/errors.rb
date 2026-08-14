# frozen_string_literal: true

module Cosmos
  module Llm
    module Evaluate
      # Base error class for all evaluation errors.
      class Error < StandardError; end

      # Raised when an evaluation is configured with invalid arguments.
      class ConfigurationError < Error; end

      # Raised when more examples fail than +max_errors+ allows.
      #
      # Carries the underlying failures so a caller can see what actually went
      # wrong rather than just the count.
      class TooManyErrorsError < Error
        # @return [Array<Hash>] the recorded failures, each with +:example+ and +:error+
        attr_reader :failures

        # @param message [String] the error message
        # @param failures [Array<Hash>] the recorded failures
        def initialize(message, failures: [])
          @failures = failures
          super(message)
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
