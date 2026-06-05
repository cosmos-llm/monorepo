# frozen_string_literal: true

# Main entry point for the Cosmos::Llm::Context module.
#
# This module provides a comprehensive DSL for modeling and managing LLM agentic contexts.
# It enables the creation of structured contexts including virtual filesystems, configurable
# context blocks, tool declarations, and other components needed for agentic LLM interactions.
#
# The module uses Zeitwerk for efficient autoloading of its components.
#
# ## Basic Usage
#
# ```ruby
# require 'cosmos/llm/context'
#
# # Create a new context
# context = Cosmos::Llm::Context.build do
#   # Add a root filesystem
#   filesystem do
#     directory 'src' do
#       file 'main.rb', content: 'puts "Hello World"'
#     end
#   end
#
#   # Add context blocks
#   block :system_prompt do
#     "You are a helpful assistant."
#   end
#
#   block :user_message do
#     "Help me with Ruby programming."
#   end
# end
#
# # Render the context
# puts context.render
# ```
#
# @see Cosmos::Llm::Context::Builder For the DSL builder
# @see Cosmos::Llm::Context::Filesystem For virtual filesystem support

require 'zeitwerk'
loader = Zeitwerk::Loader.new
loader.tag = File.basename(__FILE__, '.rb')
loader.inflector = Zeitwerk::GemInflector.new(__FILE__)
loader.push_dir("#{File.dirname(__FILE__)}/../../..")

require 'cosmos/llm/context/version'
require 'cosmos/llm/context/errors'
require 'cosmos/llm/virtual_filesystem'

module Cosmos
  module Llm
    # The Context module provides a DSL for building agentic LLM contexts.
    #
    # This module serves as the main entry point for the Cosmos LLM Context gem, offering:
    # - Virtual filesystem modeling
    # - Configurable context blocks (prompts, messages, tool declarations)
    # - Flexible rendering and serialization
    # - Context composition and nesting
    #
    # @example Building a simple context
    #   context = Cosmos::Llm::Context.build do
    #     block :system, "You are a helpful assistant"
    #     filesystem { directory('src') }
    #   end
    #
    # @see Cosmos::Llm::Context::Builder
    # @see Cosmos::Llm::Context::Filesystem
    module Context
      class << self
        # Creates a new context using the DSL builder.
        #
        # This is a convenience method that creates a new Builder instance and
        # evaluates the provided block within its context.
        #
        # @yield The block to evaluate for building the context
        # @return [Builder] A configured builder instance
        # @example Build a context with filesystem
        #   context = Cosmos::Llm::Context.build do
        #     filesystem do
        #       directory 'config' do
        #         file 'settings.yml', content: 'debug: true'
        #       end
        #     end
        #   end
        def build(&block)
          Builder.new(&block)
        end
      end
    end
  end
end

require 'cosmos/llm/context/builder'
require 'cosmos/llm/context/filesystem'
require 'cosmos/llm/context/block'
require 'cosmos/llm/context/renderers'

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
