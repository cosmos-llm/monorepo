#!/usr/bin/env ruby
# frozen_string_literal: true

require_relative '../lib/cosmos/llm/tool/preset'
require 'cosmos/llm/virtual_filesystem'
require 'json'

# Create a sample virtual filesystem
fs = Cosmos::Llm::VirtualFilesystem::Filesystem.new('project') do
  directory 'src' do
    file 'main.rb', content: <<~RUBY
      # Main application file
      def main
        puts "Hello World"
        helper_method
      end

      # TODO: Add error handling
      def helper_method
        puts "This is a helper method"
      end

      main if __FILE__ == $PROGRAM_NAME
    RUBY

    file 'calculator.rb', content: <<~RUBY
      # Calculator module
      module Calculator
        def self.add(a, b)
          a + b
        end

        def self.subtract(a, b)
          a - b
        end

        # TODO: Implement multiply and divide
      end
    RUBY

    directory 'lib' do
      file 'utils.rb', content: <<~RUBY
        # Utility functions
        module Utils
          def self.format_output(text)
            text.upcase
          end
        end
      RUBY
    end
  end

  directory 'config' do
    file 'settings.yml', content: <<~YAML
      # Application settings
      app_name: My Project
      version: 1.0.0
      debug: true
    YAML
  end

  directory 'data' do
    file 'users.json', content: JSON.pretty_generate({
                                                       users: [
                                                         { id: 1, name: 'Alice', email: 'alice@example.com',
                                                           role: 'admin' },
                                                         { id: 2, name: 'Bob', email: 'bob@example.com', role: 'user' },
                                                         { id: 3, name: 'Charlie', email: 'charlie@example.com',
                                                           role: 'user' }
                                                       ],
                                                       metadata: {
                                                         total: 3,
                                                         generated_at: '2025-01-01T00:00:00Z'
                                                       }
                                                     })
  end

  file 'README.md', content: <<~MARKDOWN
    # My Project

    This is a sample Ruby project to demonstrate the tool presets.

    ## Features

    - Main application logic
    - Calculator module
    - Utility functions

    ## TODO

    - Add tests
    - Improve documentation
  MARKDOWN
end

puts '=== Cosmos LLM Tool Preset Examples ==='
puts

# Example 1: List all files
puts '=== Example 1: List All Files ==='
list_tool = Cosmos::Llm::Tool::Preset.list(fs)
result = list_tool.call
puts "Total files: #{result[:count]}"
result[:files].each do |file|
  puts "  #{file[:path]} (#{file[:size]} bytes)"
end
puts

# Example 2: List only Ruby files
puts '=== Example 2: List Ruby Files Only ==='
result = list_tool.call(pattern: '*.rb')
puts "Ruby files found: #{result[:count]}"
result[:files].each do |file|
  puts "  #{file[:path]}"
end
puts

# Example 3: Read a specific file
puts '=== Example 3: Read src/main.rb ==='
read_tool = Cosmos::Llm::Tool::Preset.read(fs)
result = read_tool.call(file_path: 'src/main.rb')
if result[:success]
  puts "File: #{result[:file_path]}"
  puts "Lines: #{result[:start_line]}-#{result[:end_line]} of #{result[:total_lines]}"
  puts 'Content:'
  puts result[:content]
else
  puts "Error: #{result[:error]}"
end
puts

# Example 4: Read with offset and limit
puts '=== Example 4: Read First 5 Lines of src/calculator.rb ==='
result = read_tool.call(file_path: 'src/calculator.rb', offset: 0, limit: 5)
if result[:success]
  puts "Reading lines #{result[:start_line]}-#{result[:end_line]}:"
  puts result[:content]
end
puts

# Example 5: Search for TODO comments
puts '=== Example 5: Search for TODO Comments ==='
grep_tool = Cosmos::Llm::Tool::Preset.grep(fs)
result = grep_tool.call(pattern: 'TODO')
puts "Found #{result[:match_count]} matches in #{result[:files_with_matches]} files:"
result[:matches].each do |match|
  puts "  #{match[:file]}:#{match[:line_number]}: #{match[:content].strip}"
end
puts

# Example 6: Search for 'def' in Ruby files
puts '=== Example 6: Search for Method Definitions in Ruby Files ==='
result = grep_tool.call(pattern: 'def ', file_pattern: '*.rb')
puts "Found #{result[:match_count]} method definitions:"
result[:matches].each do |match|
  puts "  #{match[:file]}:#{match[:line_number]}: #{match[:match]}"
end
puts

# Example 7: Write tool (preparation)
puts '=== Example 7: Prepare File Write ==='
write_tool = Cosmos::Llm::Tool::Preset.write(fs)
result = write_tool.call(
  file_path: 'src/new_file.rb',
  content: 'puts "This is a new file"'
)
puts 'Write operation prepared:'
puts "  File: #{result[:file_path]}"
puts "  Size: #{result[:size]} bytes"
puts "  Status: #{result[:created] ? 'Will create new file' : 'Will update existing file'}"
puts

# Example 8: JQ - Query JSON from filesystem
puts '=== Example 8: Query JSON File with JQ ==='
jq_tool = Cosmos::Llm::Tool::Preset.jq(fs)
result = jq_tool.call(file_path: 'data/users.json', query: '.users')
if result[:success]
  puts "Query: #{result[:query]}"
  puts 'Result (users array):'
  puts result[:output]
end
puts

# Example 9: JQ - Extract specific fields
puts '=== Example 9: Extract User Names ==='
result = jq_tool.call(file_path: 'data/users.json', query: '.users[0].name')
puts "First user name: #{result[:result]}" if result[:success]
puts

# Example 10: JQ - Query from JSON string
puts '=== Example 10: Query JSON String ==='
json_data = '{"status": "success", "data": {"count": 42, "items": ["a", "b", "c"]}}'
result = jq_tool.call(json: json_data, query: '.data.count')
puts "Count: #{result[:result]}" if result[:success]
puts

# Example 11: JQ - Use special queries
puts '=== Example 11: JQ Special Queries ==='
result = jq_tool.call(file_path: 'data/users.json', query: 'keys')
puts "Top-level keys: #{result[:result]}" if result[:success]

result = jq_tool.call(file_path: 'data/users.json', query: '.users | length')
puts "Number of users: #{result[:result]}" if result[:success]
puts

# Example 12: Generate tool schemas for LLM providers
puts '=== Example 12: Generate Tool Schemas ==='
puts
puts 'OpenAI Schema for Read Tool:'
puts JSON.pretty_generate(read_tool.to_openai_schema)
puts
puts 'Anthropic Schema for JQ Tool:'
puts JSON.pretty_generate(jq_tool.to_anthropic_schema)
puts

# Example 13: Webfetch (requires network access)
puts '=== Example 13: Webfetch Tool (External) ==='
webfetch_tool = Cosmos::Llm::Tool::Preset.webfetch
puts 'Webfetch tool created (no filesystem required)'
puts 'Schema:'
puts JSON.pretty_generate(webfetch_tool.to_anthropic_schema)
puts

puts '=== Examples Complete ==='

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
