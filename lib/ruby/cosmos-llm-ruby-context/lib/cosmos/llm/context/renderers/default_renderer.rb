# frozen_string_literal: true

module Cosmos
  module Llm
    module Context
      module Renderers
        # Default text-based renderer.
        #
        # Renders contexts as a structured text format suitable for general use.
        class DefaultRenderer
          # Renders a context builder to a string.
          #
          # @param builder [Builder] The builder to render
          # @return [String] The rendered output
          def self.render(builder)
            parts = []

            parts << filesystem_section(builder) if builder.root_filesystem
            parts << blocks_section(builder) unless builder.blocks.empty?

            parts.join("\n")
          end

          private_class_method def self.filesystem_section(builder)
            ['=== FILESYSTEM ===', builder.root_filesystem.tree]
          end

          private_class_method def self.blocks_section(builder)
            section = ['=== CONTEXT BLOCKS ===']
            builder.blocks.each do |block|
              section << "\n[#{block.name.upcase}]"
              section << block.content.to_s
            end
            section
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
