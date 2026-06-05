# frozen_string_literal: true

require 'cosmos/llm/tool'

module Cosmos
  module Llm
    module Tool
      module Preset
        # Glob pattern matching tool (virtual filesystem)
        #
        # Provides functionality to find files matching glob patterns in the virtual
        # filesystem, similar to Unix glob or find commands.
        #
        # @example Using the glob tool
        #   tool = Cosmos::Llm::Tool::Preset.glob(filesystem)
        #   result = tool.call(pattern: '**/*.rb')
        #
        # @param filesystem [Cosmos::Llm::VirtualFilesystem::Filesystem] The virtual filesystem to search
        # @return [Cosmos::Llm::Tool::Definition] A glob pattern matching tool
        def self.glob(filesystem)
          Cosmos::Llm::Tool.define(:glob, register: false) do
            description 'Find files matching glob patterns in the virtual filesystem'

            parameter :pattern,
                      type: :string,
                      required: true,
                      description: 'Glob pattern to match files (e.g., "*.rb", "src/**/*.js", "**/*test*")'

            execute do |params|
              pattern = params[:pattern]

              begin
                # Get all files from the filesystem
                all_files = filesystem.all_files

                # Convert glob pattern to regex
                # Handle special glob characters:
                # ** - matches any number of directories
                # * - matches any characters except /
                # ? - matches single character
                # {a,b} - matches either a or b
                regex_pattern = pattern.dup

                # Escape regex special characters except glob wildcards
                regex_pattern = regex_pattern.gsub(/([.+^$|()\[\]])/) { "\\#{Regexp.last_match(1)}" }

                # Convert glob patterns to regex
                regex_pattern = regex_pattern.gsub('**/', '(.*/)*') # ** matches any directories
                regex_pattern = regex_pattern.gsub('**', '.*')     # ** at end matches anything
                regex_pattern = regex_pattern.gsub('*', '[^/]*')   # * matches anything except /
                regex_pattern = regex_pattern.gsub('?', '[^/]')    # ? matches single char

                # Handle {a,b} alternation
                regex_pattern = regex_pattern.gsub(/\{([^}]+)\}/) do
                  options = Regexp.last_match(1).split(',').map(&:strip)
                  "(#{options.join('|')})"
                end

                # Anchor the pattern
                regex_pattern = "^#{regex_pattern}$"
                regex = Regexp.new(regex_pattern)

                # Filter files matching the pattern
                matched_files = all_files.select { |file_info| regex.match?(file_info[:path]) }

                # Extract just the paths
                paths = matched_files.map { |file_info| file_info[:path] }

                {
                  success: true,
                  paths: paths,
                  count: paths.length,
                  pattern: pattern
                }
              rescue StandardError => e
                {
                  success: false,
                  error: e.message,
                  pattern: pattern
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
