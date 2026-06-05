# frozen_string_literal: true

module Cosmos
  module Llm
    module VirtualFilesystem
      # Base error class for all Cosmos::Llm::VirtualFilesystem errors.
      #
      # All custom exceptions in this module inherit from this base class,
      # allowing users to rescue all virtual filesystem-related errors with a single rescue clause.
      #
      # @example Rescue all virtual filesystem errors
      #   begin
      #     filesystem.find_file('path')
      #   rescue Cosmos::Llm::VirtualFilesystem::Error => e
      #     handle_error(e)
      #   end
      class Error < StandardError; end

      # Raised when an invalid name is provided for a filesystem node or file.
      #
      # @example
      #   VirtualFile.new(nil)  # raises InvalidNameError
      class InvalidNameError < Error; end

      # Raised when an invalid path is provided for filesystem operations.
      #
      # @example
      #   VirtualFile.new("path/with/slash")  # raises InvalidPathError
      class InvalidPathError < Error; end

      # Raised when a requested file cannot be found in the virtual filesystem.
      #
      # @example
      #   filesystem.find_file("nonexistent.txt")  # may raise FileNotFoundError
      class FileNotFoundError < Error; end

      # Raised when validation fails for input data.
      #
      # @example
      #   VirtualFile.new("", "content")  # raises ValidationError
      class ValidationError < Error; end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
