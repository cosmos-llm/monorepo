# frozen_string_literal: true

require 'json'

module Cosmos
  module Llm
    module Context
      module Renderers
        # JSON-based renderer.
        #
        # Renders contexts as JSON for API consumption or storage.
        class JsonRenderer
          # Renders a context builder to JSON.
          #
          # @param builder [Builder] The builder to render
          # @return [String] The JSON output
          def self.render(builder)
            JSON.pretty_generate(builder.to_h)
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
