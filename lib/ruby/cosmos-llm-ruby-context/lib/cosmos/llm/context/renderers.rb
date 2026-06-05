# frozen_string_literal: true

require_relative 'renderers/default_renderer'
require_relative 'renderers/xml_renderer'
require_relative 'renderers/json_renderer'
require_relative 'renderers/anthropic_renderer'
require_relative 'renderers/openai_renderer'

module Cosmos
  module Llm
    module Context
      # Namespace for context renderers.
      #
      # Renderers convert context builders into various output formats suitable
      # for different LLM providers or use cases.
      #
      # This module provides a registration system for custom renderers, allowing
      # users to extend the rendering capabilities without modifying the core library.
      #
      # @example Register a custom renderer
      #   class MyCustomRenderer
      #     def self.render(builder)
      #       # Custom rendering logic
      #     end
      #   end
      #
      #   Cosmos::Llm::Context::Renderers.register(:custom, MyCustomRenderer)
      #   context.render(:custom)
      module Renderers
        @renderers = {
          default: nil, # Lazy loaded
          xml: nil,
          json: nil,
          anthropic: nil,
          openai: nil
        }

        # Registers a custom renderer for the specified format.
        #
        # @param name [Symbol, String] The name of the renderer format
        # @param renderer_class [Class] The renderer class (must respond to .render)
        # @param overwrite [Boolean] Whether to overwrite existing registrations (default: false)
        # @return [void]
        # @raise [DuplicateRegistrationError] If format already exists and overwrite is false
        # @raise [ArgumentError] If renderer_class doesn't respond to render
        # @example Register a custom renderer
        #   Renderers.register(:markdown, MarkdownRenderer)
        def self.register(name, renderer_class, overwrite: false)
          name_sym = name.to_sym

          unless renderer_class.respond_to?(:render)
            raise ArgumentError, 'Renderer class must respond to .render method'
          end

          if @renderers.key?(name_sym) && @renderers[name_sym] && !overwrite
            raise DuplicateRegistrationError,
                  "Renderer '#{name}' is already registered. Use overwrite: true to replace."
          end

          @renderers[name_sym] = renderer_class
        end

        # Returns the appropriate renderer class for the given format.
        #
        # @param format [Symbol, String] The format name
        # @return [Class] The renderer class
        # @raise [RendererNotFoundError] If the format is not supported
        # @example Get a renderer
        #   renderer = Renderers.get_renderer(:json)
        #   output = renderer.render(builder)
        def self.get_renderer(format)
          format_sym = format.to_sym

          # Lazy load built-in renderers
          if @renderers[format_sym].nil? && builtin_renderer?(format_sym)
            @renderers[format_sym] = load_builtin_renderer(format_sym)
          end

          renderer = @renderers[format_sym]
          return renderer if renderer

          raise RendererNotFoundError, "Unknown renderer format: #{format}. " \
                                       "Available formats: #{@renderers.keys.join(', ')}"
        end

        # Lists all registered renderer formats.
        #
        # @return [Array<Symbol>] Array of registered renderer format names
        # @example List available renderers
        #   Renderers.available_formats  # => [:default, :xml, :json, :anthropic, :openai, :custom]
        def self.available_formats
          @renderers.keys
        end

        # Checks if a renderer format is registered.
        #
        # @param format [Symbol, String] The format name to check
        # @return [Boolean] True if the format is registered
        # @example Check if format exists
        #   Renderers.registered?(:json)  # => true
        def self.registered?(format)
          @renderers.key?(format.to_sym)
        end

        private_class_method def self.builtin_renderer?(format)
          %i[default xml json anthropic openai].include?(format)
        end

        private_class_method def self.load_builtin_renderer(format)
          case format
          when :default then DefaultRenderer
          when :xml then XmlRenderer
          when :json then JsonRenderer
          when :anthropic then AnthropicRenderer
          when :openai then OpenAiRenderer
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
