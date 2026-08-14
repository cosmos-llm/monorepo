# frozen_string_literal: true

module Cosmos
  module Llm
    class Signature
      # Base error class for all signature errors.
      #
      # @example Rescue any signature error
      #   begin
      #     signature.coerce(values)
      #   rescue Cosmos::Llm::Signature::Error => e
      #     warn e.message
      #   end
      class Error < StandardError; end

      # Raised when a signature is declared with invalid fields or syntax.
      #
      # @example Duplicate field names
      #   Cosmos::Llm::Signature.build do
      #     input :question
      #     output :question
      #   end # => raises DefinitionError
      class DefinitionError < Error; end

      # Raised when a value cannot be coerced to a field's declared type.
      #
      # @example Non-numeric text for a Float field
      #   field.coerce('banana') # => raises CoercionError
      class CoercionError < Error; end

      # Raised when required fields are missing from a set of values.
      class MissingFieldError < Error; end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
