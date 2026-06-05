# frozen_string_literal: true

module Cosmos
  module Llm
    module Tool
      # Base error class for tool-related errors.
      class Error < StandardError; end

      # Raised when parameter validation fails.
      class ValidationError < Error; end

      # Raised when tool execution fails.
      class ExecutionError < Error; end

      # Raised when a tool is not found.
      class ToolNotFoundError < Error; end

      # Raised when a tool has no execution handler.
      class NoHandlerError < Error; end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
