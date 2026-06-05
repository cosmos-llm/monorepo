# frozen_string_literal: true

require 'cosmos/llm/tool'

module Cosmos
  module Llm
    module Tool
      module Preset
        # Read file contents preset tool (virtual filesystem)
        #
        # Provides functionality to read file contents from a virtual filesystem
        # with options for reading specific lines or portions of files.
        #
        # @example Using the read tool
        #   tool = Cosmos::Llm::Tool::Preset.read(filesystem)
        #   result = tool.call(file_path: 'src/main.rb')
        #
        # @param filesystem [Cosmos::Llm::VirtualFilesystem::Filesystem] The virtual filesystem to operate on
        # @return [Cosmos::Llm::Tool::Definition] A read file tool
        def self.read(filesystem)
          Cosmos::Llm::Tool.define(:read, register: false) do
            description 'Read file contents from the virtual filesystem with options for specific lines and portions'

            parameter :file_path,
                      type: :string,
                      required: true,
                      description: 'The path to the file to read (relative to virtual filesystem root)'

            parameter :offset,
                      type: :number,
                      required: false,
                      description: 'The line number to start reading from (0-based)'

            parameter :limit,
                      type: :number,
                      required: false,
                      description: 'The number of lines to read (default: 2000)'

            execute do |params|
              file_path = params[:file_path]
              offset = params.fetch(:offset, 0).to_i
              limit = params.fetch(:limit, 2000).to_i

              # Find file in virtual filesystem
              virtual_file = filesystem.find_file(file_path)

              unless virtual_file
                next {
                  success: false,
                  error: 'File not found in virtual filesystem',
                  file_path: file_path
                }
              end

              begin
                content = virtual_file.content
                if content.nil?
                  next {
                    success: false,
                    error: 'File content is nil',
                    file_path: file_path
                  }
                end

                # Check if content appears to be binary
                is_binary = content.include?("\x00")

                if is_binary
                  # For binary files, return raw content without line formatting
                  formatted_content = content
                  total_lines = 0
                  read_lines = 1
                  start_line = 1
                  end_line = 1
                else
                  lines = content.lines
                  total_lines = lines.size

                  # Apply offset and limit
                  start_line = [offset, 0].max
                  end_line = start_line + limit - 1
                  selected_lines = lines[start_line..end_line] || []
                  read_lines = selected_lines.size

                  # Format lines with line numbers (1-based)
                  # Calculate width for line numbers based on total lines
                  line_number_width = [total_lines.to_s.length, 1].max
                  formatted_content = selected_lines.map.with_index(start_line + 1) do |line, line_num|
                    spaces = ' ' * (line_number_width - line_num.to_s.length)
                    "#{spaces}#{line_num}\t#{line}"
                  end.join

                  start_line += 1 # Convert to 1-based
                  end_line = read_lines.positive? ? start_line + read_lines - 1 : start_line - 1
                end

                {
                  success: true,
                  file_path: file_path,
                  content: formatted_content,
                  total_lines: total_lines,
                  read_lines: read_lines,
                  start_line: start_line,
                  end_line: end_line,
                  size: content.bytesize,
                  attributes: virtual_file.attributes
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
