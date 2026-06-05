#!/usr/bin/env ruby
# frozen_string_literal: true

require_relative '../lib/cosmos/llm/context'

# Example: Basic context with blocks and filesystem
context = Cosmos::Llm::Context.build do
  # Add a system prompt block
  block :system, 'You are a helpful Ruby programming assistant.'

  # Add a user message block
  block :user, 'Help me create a simple Ruby project structure.'

  # Define a virtual filesystem representing a Ruby project
  filesystem do
    # Root-level files
    file 'Gemfile', content: <<~GEMFILE
      source 'https://rubygems.org'

      gem 'rake', '~> 13.0'
    GEMFILE

    file 'Rakefile', content: <<~RAKEFILE
      require 'rake/testtask'

      Rake::TestTask.new(:test) do |t|
        t.libs << 'test'
        t.test_files = FileList['test/**/*_test.rb']
      end

      task default: :test
    RAKEFILE

    # Source directory
    directory 'lib' do
      file 'example.rb', content: <<~RUBY
        # Example library file
        class Example
          def greet(name)
            "Hello, \#{name}!"
          end
        end
      RUBY
    end

    # Test directory
    directory 'test' do
      file 'example_test.rb', content: <<~RUBY
        require 'minitest/autorun'
        require_relative '../lib/example'

        class ExampleTest < Minitest::Test
          def test_greet
            example = Example.new
            assert_equal 'Hello, World!', example.greet('World')
          end
        end
      RUBY
    end
  end
end

# Demonstrate different rendering formats
puts '=== DEFAULT RENDERER ==='
puts context.render
puts "\n"

puts '=== XML RENDERER ==='
puts context.render(:xml)
puts "\n"

puts '=== JSON RENDERER ==='
puts context.render(:json)
puts "\n"

puts '=== ANTHROPIC RENDERER ==='
puts context.render(:anthropic)
puts "\n"

puts '=== OPENAI RENDERER ==='
puts context.render(:openai)

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
