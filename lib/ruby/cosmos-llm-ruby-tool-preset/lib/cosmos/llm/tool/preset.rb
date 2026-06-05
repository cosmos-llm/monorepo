# frozen_string_literal: true

require 'cosmos/llm/tool'
require_relative 'preset/version'

module Cosmos
  module Llm
    module Tool
      # Pre-defined tools for cosmos-llm-tool
      #
      # This module provides a collection of ready-to-use, pre-defined tools that can be
      # easily integrated into LLM applications. Tools include file operations, search,
      # and web fetching capabilities, all designed to work with the virtual filesystem
      # for secure, sandboxed execution.
      #
      # ## Available Presets
      #
      # ### Virtual Filesystem Tools (require filesystem parameter)
      # - **read**: Read file contents with line-based slicing
      # - **write**: Write/update file contents
      # - **list**: List all files with optional pattern filtering
      # - **glob**: Find files matching glob patterns
      # - **grep**: Search for patterns using regex
      # - **jq**: Query and transform JSON data (optional filesystem for file reading)
      #
      # ### External Tools (no filesystem required)
      # - **webfetch**: Fetch content from URLs with format conversion
      #
      # ## Basic Usage
      #
      # ```ruby
      # require 'cosmos/llm/tool/preset'
      # require 'cosmos/llm/virtual_filesystem'
      #
      # # Create a virtual filesystem
      # fs = Cosmos::Llm::VirtualFilesystem::Filesystem.new('project') do
      #   directory 'src' do
      #     file 'main.rb', content: 'puts "Hello World"'
      #     file 'helper.rb', content: 'def help; end'
      #   end
      #   file 'README.md', content: '# My Project'
      # end
      #
      # # Create preset tools
      # read_tool = Cosmos::Llm::Tool::Preset.read(fs)
      # grep_tool = Cosmos::Llm::Tool::Preset.grep(fs)
      # webfetch_tool = Cosmos::Llm::Tool::Preset.webfetch
      #
      # # Use the tools
      # result = read_tool.call(file_path: 'src/main.rb')
      # puts result[:content]
      #
      # search_result = grep_tool.call(pattern: 'def', file_pattern: '*.rb')
      # puts search_result[:matches]
      #
      # web_result = webfetch_tool.call(url: 'https://example.com', format: 'markdown')
      # puts web_result[:content]
      # ```
      #
      # ## Using with LLM Providers
      #
      # ```ruby
      # # Generate schemas for Anthropic
      # tools_for_anthropic = [
      #   read_tool.to_anthropic_schema,
      #   grep_tool.to_anthropic_schema,
      #   webfetch_tool.to_anthropic_schema
      # ]
      #
      # # Generate schemas for OpenAI
      # tools_for_openai = [
      #   read_tool.to_openai_schema,
      #   grep_tool.to_openai_schema,
      #   webfetch_tool.to_openai_schema
      # ]
      # ```
      #
      # @see Cosmos::Llm::Tool::Definition
      # @see Cosmos::Llm::VirtualFilesystem::Filesystem
      module Preset
        autoload :Read,         'cosmos/llm/tool/preset/read'
        autoload :Write,        'cosmos/llm/tool/preset/write'
        autoload :List,         'cosmos/llm/tool/preset/list'
        autoload :Glob,         'cosmos/llm/tool/preset/glob'
        autoload :Grep,         'cosmos/llm/tool/preset/grep'
        autoload :Jq,           'cosmos/llm/tool/preset/jq'
        autoload :Webfetch,     'cosmos/llm/tool/preset/webfetch'
        autoload :Diff,         'cosmos/llm/tool/preset/diff'
        autoload :Patch,        'cosmos/llm/tool/preset/patch'
        autoload :ExecContainer, 'cosmos/llm/tool/preset/exec'
      end
    end
  end
end

# Require all preset files
require_relative 'preset/read'
require_relative 'preset/write'
require_relative 'preset/list'
require_relative 'preset/glob'
require_relative 'preset/grep'
require_relative 'preset/jq'
require_relative 'preset/webfetch'
require_relative 'preset/diff'
require_relative 'preset/patch'
require_relative 'preset/exec'

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
