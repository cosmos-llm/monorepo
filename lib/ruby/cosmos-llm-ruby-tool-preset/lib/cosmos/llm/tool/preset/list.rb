# frozen_string_literal: true

require 'cosmos/llm/tool'

# Cosmos LLM Tool Preset Module
#
# This module provides pre-defined tools for LLM applications, including
# file system operations like listing files with glob pattern support.
module Cosmos
  module Llm
    module Tool
      # Preset tools for common LLM operations
      module Preset
        # List files preset tool (virtual filesystem)
        #
        # Provides functionality to list all files in the virtual filesystem
        # with their paths and metadata.
        #
        # @example Using the list tool
        #   tool = Cosmos::Llm::Tool::Preset.list(filesystem)
        #   result = tool.call()
        #
        # @param filesystem [Cosmos::Llm::VirtualFilesystem::Filesystem] The virtual filesystem to list
        # @return [Cosmos::Llm::Tool::Definition] A list files tool
        def self.list(filesystem)
          Cosmos::Llm::Tool.define(:list, register: false) do
            description 'List all files in the virtual filesystem with their paths and metadata'

            parameter :pattern,
                      type: :string,
                      required: false,
                      description: 'Optional glob pattern to filter files (e.g., "*.rb", "src/**/*.js")'

            execute do |params|
              pattern = params[:pattern]

              begin
                # Get all files from the filesystem
                all_files = filesystem.all_files

                # Filter by pattern if provided
                filtered_files = if pattern
                                   # Use Ruby's File.fnmatch for proper glob pattern matching with extended globbing
                                   all_files.select { |file_info| File.fnmatch?(pattern, file_info[:path], File::FNM_EXTGLOB) }
                                 else
                                   all_files
                                 end

                # Format file information
                files = filtered_files.map do |file_info|
                  file = file_info[:file]
                  {
                    path: file_info[:path],
                    name: file.name,
                    size: file.content&.bytesize || 0,
                    attributes: file.attributes
                  }
                end

                {
                  success: true,
                  files: files,
                  count: files.length,
                  pattern: pattern
                }
              rescue StandardError => e
                {
                  success: false,
                  error: e.message
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
