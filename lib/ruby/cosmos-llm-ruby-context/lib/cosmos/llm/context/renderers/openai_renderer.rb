# frozen_string_literal: true

module Cosmos
  module Llm
    module Context
      module Renderers
        # OpenAI-specific renderer.
        #
        # Renders contexts in a format optimized for OpenAI models.
        class OpenAiRenderer
          # Renders a context builder for OpenAI.
          #
          # @param builder [Builder] The builder to render
          # @return [String] The formatted output
          def self.render(builder)
            output = []

            # Render all blocks in order
            builder.blocks.each do |block|
              case block.name.to_sym
              when :system, :user, :assistant
                output << "#{block.name.to_s.upcase}:"
                output << block.content.to_s
                output << ''
              else
                output << block.content.to_s
              end
            end

            # Render filesystem as markdown
            if builder.root_filesystem
              output << '## Project Structure'
              output << '```'
              output << builder.root_filesystem.tree.strip
              output << '```'
              output << ''

              all_files = builder.root_filesystem.all_files
              all_files.each do |entry|
                file = entry[:file]
                next unless file.content && !file.content.empty?

                output << "### #{entry[:path].gsub(%r{//+}, '/')}"
                output << '```'
                output << file.content
                output << '```'
                output << ''
              end
            end

            output.join("\n").strip
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
