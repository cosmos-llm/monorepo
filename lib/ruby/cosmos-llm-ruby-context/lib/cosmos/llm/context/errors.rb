# frozen_string_literal: true

module Cosmos
  module Llm
    module Context
      # Base error class for all Cosmos::Llm::Context errors.
      #
      # All custom exceptions in this module inherit from this base class,
      # allowing users to rescue all context-related errors with a single rescue clause.
      #
      # @example Rescue all context errors
      #   begin
      #     context.build { ... }
      #   rescue Cosmos::Llm::Context::Error => e
      #     handle_error(e)
      #   end
      class Error < StandardError; end

      # Raised when an invalid name is provided for a block or other named entity.
      #
      # @example
      #   Block.new(nil, "content")  # raises InvalidNameError
      class InvalidNameError < Error; end

      # Raised when an unknown or unregistered renderer format is requested.
      #
      # @example
      #   builder.render(:unknown_format)  # raises RendererNotFoundError
      class RendererNotFoundError < Error; end

      # Raised when validation fails for input data.
      #
      # @example
      #   Block.new(:name, "")  # may raise ValidationError
      class ValidationError < Error; end

      # Raised when a duplicate registration is attempted.
      #
      # @example
      #   Renderers.register(:json, CustomRenderer)
      #   Renderers.register(:json, AnotherRenderer)  # raises DuplicateRegistrationError
      class DuplicateRegistrationError < Error; end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
