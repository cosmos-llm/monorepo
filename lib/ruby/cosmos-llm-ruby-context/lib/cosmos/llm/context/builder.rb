# frozen_string_literal: true

require_relative 'block'
require_relative 'renderers'

module Cosmos
  module Llm
    module Context
      # Builder class for constructing LLM contexts using a DSL.
      #
      # This class provides a fluent interface for building complex agentic contexts
      # through method chaining and block evaluation. It manages filesystems, context
      # blocks, and provides rendering capabilities.
      #
      # @example Basic usage
      #   builder = Cosmos::Llm::Context::Builder.new do
      #     block :system, "You are a helpful assistant"
      #     filesystem { directory('src') }
      #   end
      #
      # @see Cosmos::Llm::Context
      class Builder
        # Load and include all builder mixins dynamically
        mixins_path = File.join(File.dirname(__FILE__), 'builder_mixins', '*.rb')
        Dir.glob(mixins_path).sort.each do |file|
          require file
        end
        include BuilderMixins::ContentMethods

        # @return [Array<Block>] The context blocks in this builder
        attr_reader :blocks

        # @return [Filesystem, nil] The root filesystem for this context
        attr_reader :root_filesystem

        # Initializes a new Builder instance.
        #
        # @yield Evaluates the block in the context of the builder
        # @return [Builder] A new builder instance
        def initialize(&block)
          @blocks = []
          @root_filesystem = nil
          instance_eval(&block) if block_given?
        end

        # Adds a context block to the builder.
        #
        # Context blocks represent different types of content that can be included
        # in the final context, such as system prompts, user messages, tool declarations,
        # or arbitrary string content.
        #
        # @param name [Symbol, String] The name/type of the block
        # @param content [String, nil] The content for the block
        # @param metadata [Hash] Optional metadata for the block
        # @yield Optional block that will be evaluated to generate content
        # @return [Block] The created block
        # @example Add a system prompt block
        #   block :system, "You are a helpful assistant"
        # @example Add a block with dynamic content
        #   block :user_message do
        #     "The current time is #{Time.now}"
        #   end
        # @example Add a block with metadata
        #   block :system, "You are helpful", role: 'system', priority: 1
        def block(name, content = nil, metadata = {}, &block_content)
          content_value = block_content ? block_content.call : content
          ctx_block = Block.new(name, content_value, metadata)
          @blocks << ctx_block
          ctx_block
        end

        # Defines or accesses the root filesystem for this context.
        #
        # The filesystem provides a virtual file structure that can be used to
        # represent project files, configuration, or other hierarchical data.
        #
        # @yield Block to configure the filesystem
        # @return [Filesystem] The root filesystem instance
        # @example Define a filesystem
        #   filesystem do
        #     directory 'src' do
        #       file 'main.rb', content: 'puts "Hello"'
        #     end
        #   end
        def filesystem(&block)
          @root_filesystem ||= Filesystem.new('/')
          @root_filesystem.instance_eval(&block) if block_given?
          @root_filesystem
        end

        # Renders the context to a string representation.
        #
        # @param renderer [Symbol] The renderer to use (:default, :xml, :json)
        # @return [String] The rendered context
        # @example Render to default format
        #   builder.render
        # @example Render to JSON
        #   builder.render(:json)
        def render(renderer = :default)
          renderer_class = Renderers.get_renderer(renderer)
          renderer_class.render(self)
        end

        # Converts the context to a hash representation.
        #
        # @return [Hash] Hash containing blocks and filesystem
        def to_h
          {
            blocks: @blocks.map(&:to_h),
            filesystem: @root_filesystem&.to_h
          }
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
