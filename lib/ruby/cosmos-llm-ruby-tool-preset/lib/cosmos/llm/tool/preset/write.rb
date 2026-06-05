# frozen_string_literal: true

require 'cosmos/llm/tool'

module Cosmos
  module Llm
    module Tool
      module Preset
        # Write file contents preset tool (virtual filesystem)
        #
        # Provides functionality to prepare file writes for a virtual filesystem.
        # Returns the content and metadata needed to update the virtual filesystem.
        #
        # This tool validates the write operation parameters and checks for existing files
        # in the virtual filesystem, then returns structured data containing the write
        # operation details. The actual filesystem modification must be performed by the caller.
        #
        # Key features:
        # - Validates input parameters (file_path and content)
        # - Checks for existing files to determine create vs update operation
        # - Returns comprehensive metadata including size calculations
        # - Handles binary and text content equally
        #
        # @example Writing a new Ruby file
        #   tool = Cosmos::Llm::Tool::Preset.write(filesystem)
        #   result = tool.call(file_path: 'src/new.rb', content: 'puts "hello"')
        #   # result[:success] => true
        #   # result[:created] => true
        #   # result[:size] => 13
        #
        # @example Updating an existing file
        #   tool = Cosmos::Llm::Tool::Preset.write(filesystem)
        #   result = tool.call(file_path: 'existing.txt', content: 'new content')
        #   # result[:success] => true
        #   # result[:updated] => true
        #   # result[:previous_size] => 11 (size of old content)
        #
        # @param filesystem [Cosmos::Llm::VirtualFilesystem::Filesystem] The virtual filesystem to validate against
        # @return [Cosmos::Llm::Tool::Definition] A write file tool definition
        # @raise [ArgumentError] If filesystem parameter is invalid
        # @raise [StandardError] If an error occurs during file lookup or validation
        def self.write(filesystem)
          Cosmos::Llm::Tool.define(:write, register: false) do
            description 'Write content to a file in the virtual filesystem, creating or updating as needed'

            parameter :file_path,
                      type: :string,
                      required: true,
                      description: 'The path to the file to write (relative to virtual filesystem root). Must be a non-empty string.'

            parameter :content,
                      type: :string,
                      required: true,
                      description: 'The content to write to the file. Must be a string (supports both text and binary content).'

            execute do |params|
              file_path = params[:file_path]
              content = params[:content]

              # Input validation
              if file_path.nil? || !file_path.is_a?(String) || file_path.empty?
                raise ArgumentError,
                      'file_path parameter is required and must be a non-empty string'
              end
              raise ArgumentError, 'content parameter must be a string' unless content.is_a?(String)

              begin
                # Check if file already exists in virtual filesystem
                existing_file = filesystem.find_file(file_path)

                {
                  success: true,
                  file_path: file_path,
                  content: content,
                  size: content.bytesize,
                  created: existing_file.nil?,
                  updated: !existing_file.nil?,
                  previous_size: existing_file&.content&.bytesize
                }
              rescue StandardError => e
                {
                  success: false,
                  error: e.message,
                  file_path: file_path
                }
              end
            end
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
