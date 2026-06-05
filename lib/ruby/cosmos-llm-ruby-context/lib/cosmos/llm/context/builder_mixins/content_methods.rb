# frozen_string_literal: true

module Cosmos
  module Llm
    module Context
      module BuilderMixins
        # Provides content-related methods for the Builder class.
        #
        # This mixin adds convenience methods for adding different types of content
        # to the context, including strings and files from the filesystem.
        #
        # @example Using string content
        #   builder.string("This is some context information")
        #
        # @example Loading file content
        #   builder.file_content('/path/to/file.rb', name: :source_code)
        module ContentMethods
          # Adds a string content block to the context.
          #
          # This is a convenience method for adding plain string content.
          #
          # @param content [String] The string content to add
          # @return [Builder] Returns self for method chaining
          # @example Add string content
          #   builder.string("This is some context information")
          def string(content)
            block(:string, content)
            self
          end

          # Adds a file content block from the actual filesystem.
          #
          # This reads a file from disk and adds its content as a context block.
          #
          # @param path [String] The path to the file to read
          # @param name [Symbol, String, nil] Optional name for the block
          # @return [Builder] Returns self for method chaining
          # @raise [Errno::ENOENT] If the file doesn't exist
          # @raise [ValidationError] If the path is invalid
          # @example Add a file from disk
          #   builder.file_content('/path/to/file.rb', name: :source_code)
          def file_content(path, name: nil)
            validate_file_path!(path)
            content = File.read(path)
            block_name = name || File.basename(path).to_sym
            block(block_name, content)
            self
          end

          private

          # Validates that a file path exists and is readable.
          #
          # @param path [String] The file path to validate
          # @return [void]
          # @raise [ValidationError] If the path is nil, empty, or not a string
          # @raise [Errno::ENOENT] If the file doesn't exist
          def validate_file_path!(path)
            raise ValidationError, 'File path cannot be nil' if path.nil?
            raise ValidationError, 'File path must be a String' unless path.is_a?(String)

            stripped_path = path.strip
            raise ValidationError, 'File path cannot be empty' if stripped_path.empty?
            raise Errno::ENOENT, "File not found: #{path}" unless File.exist?(stripped_path)
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
