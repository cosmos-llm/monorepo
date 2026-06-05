# frozen_string_literal: true

module Cosmos
  module Llm
    module Context
      module Renderers
        # XML-based renderer.
        #
        # Renders contexts as XML suitable for Anthropic Claude and similar models.
        class XmlRenderer
          # Renders a context builder to XML.
          #
          # @param builder [Builder] The builder to render
          # @return [String] The XML output
          def self.render(builder)
            output = ['<context>']

            output << render_filesystem(builder.root_filesystem) if builder.root_filesystem

            builder.blocks.each do |block|
              output << render_block(block)
            end

            output << '</context>'
            output.join("\n")
          end

          # Renders a filesystem node to XML.
          #
          # @param fs [Filesystem] The filesystem to render
          # @param indent [Integer] Indentation level
          # @return [String] The XML representation
          def self.render_filesystem(filesystem, indent = 1)
            ind = '  ' * indent
            output = ["#{ind}<filesystem name=\"#{escape_xml(filesystem.name)}\">"]

            filesystem.files.each do |file|
              output << "#{ind}  <file name=\"#{escape_xml(file.name)}\">"
              output << "#{ind}    #{escape_xml(file.content)}" if file.content
              output << "#{ind}  </file>"
            end

            filesystem.children.each do |child|
              output << render_filesystem(child, indent + 1)
            end

            output << "#{ind}</filesystem>"
            output.join("\n")
          end

          # Renders a block to XML.
          #
          # @param block [Block] The block to render
          # @return [String] The XML representation
          def self.render_block(block)
            output = ["  <block type=\"#{escape_xml(block.name.to_s)}\">"]
            output << "    #{escape_xml(block.content.to_s)}"
            output << '  </block>'
            output.join("\n")
          end

          # Escapes XML special characters.
          #
          # @param str [String] The string to escape
          # @return [String] The escaped string
          def self.escape_xml(str)
            str.to_s
               .gsub('&', '&amp;')
               .gsub('<', '&lt;')
               .gsub('>', '&gt;')
               .gsub('"', '&quot;')
               .gsub("'", '&apos;')
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
