# frozen_string_literal: true

# Main entry point for the Cosmos::Llm::VirtualFilesystem module.
#
# This module provides virtual filesystem support for LLM agentic contexts.
# It enables the creation of hierarchical file and directory structures that can
# be used to organize code, configuration, and other content within LLM contexts.
#
# The module uses Zeitwerk for efficient autoloading of its components.
#
# ## Basic Usage
#
# ```ruby
# require 'cosmos/llm/virtual_filesystem'
#
# # Create a virtual filesystem
# fs = Cosmos::Llm::VirtualFilesystem::Filesystem.new('/') do
#   directory 'src' do
#     file 'main.rb', content: 'puts "Hello World"'
#     directory 'lib' do
#       file 'helper.rb', content: 'module Helper; end'
#     end
#   end
# end
#
# # Navigate and query the filesystem
# puts fs.tree
# file = fs.find_file('src/main.rb')
# puts file.content
# ```
#
# @see Cosmos::Llm::VirtualFilesystem::Filesystem
# @see Cosmos::Llm::VirtualFilesystem::VirtualFile

require 'zeitwerk'
loader = Zeitwerk::Loader.new
loader.tag = File.basename(__FILE__, '.rb')
loader.inflector = Zeitwerk::GemInflector.new(__FILE__)
loader.push_dir("#{File.dirname(__FILE__)}/../../..")

require 'cosmos/llm/virtual_filesystem/version'
require 'cosmos/llm/virtual_filesystem/errors'

module Cosmos
  module Llm
    # The VirtualFilesystem module provides hierarchical file and directory structures
    # for LLM contexts.
    #
    # This module serves as the main entry point for the Cosmos LLM Virtual Filesystem gem, offering:
    # - Virtual directory and file modeling
    # - Hierarchical structure with nested directories
    # - File content and metadata management
    # - Path-based file lookup
    # - Tree visualization
    #
    # @example Creating a simple filesystem
    #   fs = Cosmos::Llm::VirtualFilesystem::Filesystem.new('project') do
    #     file 'README.md', content: '# My Project'
    #     directory 'src' do
    #       file 'main.rb'
    #     end
    #   end
    #
    # @see Cosmos::Llm::VirtualFilesystem::Filesystem
    # @see Cosmos::Llm::VirtualFilesystem::VirtualFile
    module VirtualFilesystem
    end
  end
end

require 'cosmos/llm/virtual_filesystem/filesystem'

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
