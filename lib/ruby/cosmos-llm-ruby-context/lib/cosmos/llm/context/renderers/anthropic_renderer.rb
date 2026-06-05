# frozen_string_literal: true

module Cosmos
  module Llm
    module Context
      module Renderers
        # Anthropic-specific renderer.
        #
        # Renders contexts in a format optimized for Anthropic Claude models.
        class AnthropicRenderer
          # Renders a context builder for Anthropic Claude.
          #
          # @param builder [Builder] The builder to render
          # @return [String] The formatted output
          def self.render(builder)
            # System blocks come first
            system_blocks = builder.blocks.select { |b| b.type?(:system) }
            output = system_blocks.map do |block|
              block.content.to_s
            end

            # Then filesystem if present
            if builder.root_filesystem
              output << "\n<filesystem>"
              output << render_filesystem(builder.root_filesystem)
              output << '</filesystem>'
            end

            # Then other blocks
            other_blocks = builder.blocks.reject { |b| b.type?(:system) }
            other_blocks.each do |block|
              output << "\n<#{block.name}>"
              output << block.content.to_s
              output << "</#{block.name}>"
            end

            output.join("\n")
          end

          # Renders filesystem in Anthropic format.
          #
          # @param filesystem [Filesystem] The filesystem to render
          # @param path [String] Current path
          # @return [String] The formatted filesystem
          def self.render_filesystem(filesystem, path = '')
            output = []

            # Determine current path prefix
            current_path = if path.empty?
                             # Root node: use empty prefix if name is '/', otherwise use name with /
                             filesystem.name == '/' ? '' : "/#{filesystem.name}"
                           else
                             # Normal case: append to parent prefix
                             "#{path}/#{filesystem.name}"
                           end

            # Render files
            filesystem.files.each do |file|
              file_path = current_path.empty? ? "/#{file.name}" : "#{current_path}/#{file.name}"
              if file.content && !file.content.empty?
                output << "\n<file path=\"#{file_path}\">"
                output << file.content
                output << '</file>'
              else
                output << "<file path=\"#{file_path}\" />"
              end
            end

            # Recursively render child directories
            filesystem.children.each do |child|
              output << render_filesystem(child, current_path)
            end

            output.join("\n")
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
