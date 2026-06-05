# frozen_string_literal: true

require 'cosmos/llm/tool'

module Cosmos
  module Llm
    module Tool
      module Preset
        # Grep (search) preset tool (virtual filesystem)
        #
        # Provides functionality to search for patterns in files within the
        # virtual filesystem using regular expressions.
        #
        # @example Using the grep tool
        #   tool = Cosmos::Llm::Tool::Preset.grep(filesystem)
        #   result = tool.call(pattern: 'TODO', file_pattern: '*.rb')
        #
        # @param filesystem [Cosmos::Llm::VirtualFilesystem::Filesystem] The virtual filesystem to search
        # @return [Cosmos::Llm::Tool::Definition] A grep search tool
        def self.grep(filesystem)
          Cosmos::Llm::Tool.define(:grep, register: false) do
            description 'Search for patterns in files using regular expressions within the virtual filesystem'

            parameter :pattern,
                      type: :string,
                      required: true,
                      description: 'The regex pattern to search for in file contents'

            parameter :file_pattern,
                      type: :string,
                      required: false,
                      description: 'File pattern to include in search (e.g., "*.rb", "*.{js,ts}")'

            execute do |params|
              search_pattern = params[:pattern]
              file_pattern = params[:file_pattern]

              begin
                regex = Regexp.new(search_pattern)

                # Get all files
                all_files = filesystem.all_files

                # Filter by file pattern if provided
                files_to_search = if file_pattern
                                    # Convert glob pattern to regex (simple conversion for search)
                                    regex_pattern = file_pattern.dup
                                    regex_pattern = regex_pattern.gsub(/([.+^$|()\[\]])/) do
                                      "\\#{Regexp.last_match(1)}"
                                    end
                                    regex_pattern = regex_pattern.gsub('*', '.*')
                                    regex_pattern = regex_pattern.gsub('?', '.')
                                    # Handle {a,b} alternation
                                    regex_pattern = regex_pattern.gsub(/\{([^}]+)\}/) do
                                      options = Regexp.last_match(1).split(',')
                                      "(#{options.join('|')})"
                                    end
                                    file_regex = Regexp.new(regex_pattern)
                                    all_files.select { |file_info| file_regex.match?(file_info[:path]) }
                                  else
                                    all_files
                                  end

                # Search through files
                matches = []
                files_searched = 0
                files_with_matches = 0

                files_to_search.each do |file_info|
                  file = file_info[:file]
                  path = file_info[:path]
                  content = file.content

                  # Skip binary files or files without content
                  next unless content
                  next if content.include?("\x00")

                  files_searched += 1
                  lines = content.lines
                  file_has_matches = false

                  lines.each_with_index do |line, index|
                    line.scan(regex) do
                      file_has_matches = true
                      matches << {
                        file: path,
                        line_number: index + 1,
                        content: line.chomp,
                        match: Regexp.last_match[0]
                      }
                    end
                  end

                  files_with_matches += 1 if file_has_matches
                end

                {
                  success: true,
                  pattern: search_pattern,
                  file_pattern: file_pattern,
                  matches: matches,
                  match_count: matches.length,
                  files_searched: files_searched,
                  files_with_matches: files_with_matches
                }
              rescue RegexpError => e
                {
                  success: false,
                  error: "Invalid regex pattern: #{e.message}",
                  pattern: search_pattern
                }
              rescue StandardError => e
                {
                  success: false,
                  error: e.message,
                  pattern: search_pattern
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
