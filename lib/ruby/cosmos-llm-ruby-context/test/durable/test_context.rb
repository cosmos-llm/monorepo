# frozen_string_literal: true

require 'test_helper'

module Cosmos
  module Llm
    class TestContext < Minitest::Test
      def test_build_simple_context
        context = Context.build do
          block :system, 'You are a helpful assistant'
          block :user, 'Hello!'
        end

        assert_equal 2, context.blocks.length
        assert_equal :system, context.blocks[0].name
        assert_equal 'You are a helpful assistant', context.blocks[0].content
      end

      def test_filesystem_creation
        context = Context.build do
          filesystem do
            directory 'src' do
              file 'main.rb', content: 'puts "Hello"'
            end
          end
        end

        refute_nil context.root_filesystem
        assert_equal '/', context.root_filesystem.name
        assert_equal 1, context.root_filesystem.children.length
        assert_equal 'src', context.root_filesystem.children[0].name
      end

      def test_nested_filesystem
        context = Context.build do
          filesystem do
            directory 'src' do
              directory 'lib' do
                file 'helper.rb', content: 'def help; end'
              end
              file 'main.rb', content: 'require_relative "lib/helper"'
            end
          end
        end

        src_dir = context.root_filesystem.children[0]
        assert_equal 2, src_dir.children.length + src_dir.files.length

        lib_dir = src_dir.children.find { |c| c.name == 'lib' }
        refute_nil lib_dir
        assert_equal 1, lib_dir.files.length
      end

      def test_default_renderer
        context = Context.build do
          block :system, 'Test system prompt'
          filesystem do
            file 'test.rb', content: 'puts "test"'
          end
        end

        output = context.render
        assert_includes output, '=== FILESYSTEM ==='
        assert_includes output, '=== CONTEXT BLOCKS ==='
        assert_includes output, 'Test system prompt'
      end

      def test_json_renderer
        context = Context.build do
          block :system, 'Test'
        end

        output = context.render(:json)
        refute_nil output
        assert_instance_of String, output

        parsed = JSON.parse(output)
        assert_equal 1, parsed['blocks'].length
      end

      def test_xml_renderer
        context = Context.build do
          block :system, 'Test & <content>'
        end

        output = context.render(:xml)
        assert_includes output, '<context>'
        assert_includes output, 'Test &amp; &lt;content&gt;'
      end

      def test_to_h
        context = Context.build do
          block :test, 'content'
          filesystem { file 'test.txt' }
        end

        hash = context.to_h
        assert_equal 1, hash[:blocks].length
        refute_nil hash[:filesystem]
      end

      def test_build_without_block
        context = Context.build
        assert_instance_of Cosmos::Llm::Context::Builder, context
        assert_empty context.blocks
        assert_nil context.root_filesystem
      end

      def test_build_with_empty_block
        context = Context.build {} # rubocop:disable Lint/EmptyBlock
        assert_instance_of Cosmos::Llm::Context::Builder, context
        assert_empty context.blocks
        assert_nil context.root_filesystem
      end

      def test_build_returns_builder_instance
        context = Context.build do
          block :system, 'test'
        end
        assert_instance_of Cosmos::Llm::Context::Builder, context
      end

      def test_block_with_content_only
        context = Context.build do
          block :system, 'Hello world'
        end

        assert_equal 1, context.blocks.length
        block = context.blocks.first
        assert_equal :system, block.name
        assert_equal 'Hello world', block.content
        assert_empty block.metadata
      end

      def test_block_with_block_content
        context = Context.build do
          block :user do
            "Dynamic content: #{Time.now.year}"
          end
        end

        assert_equal 1, context.blocks.length
        block = context.blocks.first
        assert_equal :user, block.name
        assert_includes block.content, 'Dynamic content'
        assert_includes block.content, Time.now.year.to_s
      end

      def test_block_with_both_content_and_block
        context = Context.build do
          block :assistant, 'ignored content' do
            'Dynamic content from block'
          end
        end

        assert_equal 1, context.blocks.length
        block = context.blocks.first
        assert_equal :assistant, block.name
        assert_equal 'Dynamic content from block', block.content # Block takes precedence
      end

      def test_multiple_blocks
        context = Context.build do
          block :system, 'You are helpful'
          block :user, 'Hello'
          block :assistant, 'Hi there'
        end

        assert_equal 3, context.blocks.length
        assert_equal %i[system user assistant], context.blocks.map(&:name)
        assert_equal ['You are helpful', 'Hello', 'Hi there'], context.blocks.map(&:content)
      end

      def test_filesystem_without_block
        context = Context.build do
          filesystem
        end

        refute_nil context.root_filesystem
        assert_equal '/', context.root_filesystem.name
      end

      def test_filesystem_with_block
        context = Context.build do
          filesystem do
            file 'config.yml'
          end
        end

        refute_nil context.root_filesystem
        assert_equal 1, context.root_filesystem.files.length
        assert_equal 'config.yml', context.root_filesystem.files.first.name
      end

      def test_render_invalid_format
        context = Context.build do
          block :test, 'content'
        end

        assert_raises Cosmos::Llm::Context::RendererNotFoundError do
          context.render(:invalid_format)
        end
      end

      def test_render_anthropic_format
        context = Context.build do
          block :system, 'You are a helpful assistant'
          block :user, 'Hello'
        end

        output = context.render(:anthropic)
        refute_nil output
        assert_instance_of String, output
      end

      def test_render_openai_format
        context = Context.build do
          block :system, 'You are a helpful assistant'
          block :user, 'Hello'
        end

        output = context.render(:openai)
        refute_nil output
        assert_instance_of String, output
      end

      def test_to_h_empty_context
        context = Context.build
        hash = context.to_h
        assert_equal({ blocks: [], filesystem: nil }, hash)
      end

      def test_to_h_with_blocks_only
        context = Context.build do
          block :test, 'content'
        end

        hash = context.to_h
        assert_equal 1, hash[:blocks].length
        assert_nil hash[:filesystem]
      end

      def test_to_h_with_filesystem_only
        context = Context.build do
          filesystem { file 'test.txt' }
        end

        hash = context.to_h
        assert_empty hash[:blocks]
        refute_nil hash[:filesystem]
      end

      def test_context_with_complex_structure
        context = Context.build do
          block :system, 'System prompt'
          block :user, 'User message'

          filesystem do
            directory 'src' do
              file 'main.rb', content: 'puts "hello"'
              directory 'lib' do
                file 'utils.rb', content: 'module Utils; end'
              end
            end
            file 'README.md', content: '# Project'
          end
        end

        assert_equal 2, context.blocks.length
        refute_nil context.root_filesystem

        # Test rendering works
        output = context.render
        assert_includes output, 'System prompt'
        assert_includes output, 'User message'
        assert_includes output, 'main.rb'
        assert_includes output, 'utils.rb'
      end

      def test_block_with_metadata
        context = Context.build do
          block :system, 'Content', role: 'system', priority: 1
        end

        block = context.blocks.first
        assert_equal :system, block.name
        assert_equal 'Content', block.content
        assert_equal({ role: 'system', priority: 1 }, block.metadata)
      end

      def test_empty_filesystem_rendering
        context = Context.build do
          filesystem
        end

        output = context.render
        assert_includes output, 'FILESYSTEM'
        # Should still render even with empty filesystem
      end

      def test_context_equality
        context1 = Context.build do
          block :test, 'content'
        end

        context2 = Context.build do
          block :test, 'content'
        end

        # Builders are not equal even with same content
        refute_equal context1, context2
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
