# frozen_string_literal: true

require 'test_helper'

module Cosmos
  module Llm
    module Context
      class TestBuilder < Minitest::Test
        def setup
          @builder = Builder.new
        end

        def test_initialize_empty_builder
          assert_empty @builder.blocks
          assert_nil @builder.root_filesystem
        end

        def test_initialize_with_block
          builder = Builder.new do
            block :system, 'Test content'
          end

          assert_equal 1, builder.blocks.length
          assert_equal :system, builder.blocks.first.name
        end

        def test_add_simple_block
          result = @builder.block(:test, 'content')

          assert_equal 1, @builder.blocks.length
          assert_equal :test, result.name
          assert_equal 'content', result.content
        end

        def test_add_block_with_block_argument
          @builder.block(:dynamic) do
            'generated content'
          end

          assert_equal 1, @builder.blocks.length
          assert_equal 'generated content', @builder.blocks.first.content
        end

        def test_add_multiple_blocks
          @builder.block(:system, 'System message')
          @builder.block(:user, 'User message')
          @builder.block(:assistant, 'Assistant message')

          assert_equal 3, @builder.blocks.length
          assert_equal :system, @builder.blocks[0].name
          assert_equal :user, @builder.blocks[1].name
          assert_equal :assistant, @builder.blocks[2].name
        end

        def test_add_block_with_metadata
          block = @builder.block(:system, 'System message', role: 'system', priority: 1)

          assert_equal 1, @builder.blocks.length
          assert_equal :system, block.name
          assert_equal 'System message', block.content
          assert_equal({ role: 'system', priority: 1 }, block.metadata)
        end

        def test_add_block_with_nil_content
          block = @builder.block(:empty, nil)

          assert_equal 1, @builder.blocks.length
          assert_nil block.content
        end

        def test_add_block_with_empty_content
          block = @builder.block(:empty, '')

          assert_equal 1, @builder.blocks.length
          assert_equal '', block.content
        end

        def test_add_block_with_content_and_block_prioritizes_block
          block = @builder.block(:test, 'ignored content') { 'block content' }

          assert_equal 1, @builder.blocks.length
          assert_equal 'block content', block.content
        end

        def test_filesystem_creation
          fs = @builder.filesystem

          refute_nil fs
          assert_equal '/', fs.name
          assert_same fs, @builder.root_filesystem
          assert_same fs, @builder.filesystem # Should return same instance
        end

        def test_filesystem_with_block
          @builder.filesystem do
            directory 'src'
            file 'README.md'
          end

          assert_equal 1, @builder.root_filesystem.children.length
          assert_equal 1, @builder.root_filesystem.files.length
        end

        def test_filesystem_idempotent
          fs1 = @builder.filesystem
          fs2 = @builder.filesystem

          assert_same fs1, fs2
        end

        def test_string_convenience_method
          result = @builder.string('Test string content')

          assert_equal 1, @builder.blocks.length
          assert_equal :string, @builder.blocks.first.name
          assert_equal 'Test string content', @builder.blocks.first.content
          assert_same @builder, result
        end

        def test_file_content_method
          # Create a temporary file
          require 'tempfile'
          file = Tempfile.new('test')
          file.write('Test file content')
          file.close

          @builder.file_content(file.path)

          assert_equal 1, @builder.blocks.length
          assert_equal 'Test file content', @builder.blocks.first.content

          file.unlink
        end

        def test_file_content_with_custom_name
          require 'tempfile'
          file = Tempfile.new('test')
          file.write('Content')
          file.close

          result = @builder.file_content(file.path, name: :custom_name)

          assert_equal :custom_name, @builder.blocks.first.name
          assert_same @builder, result

          file.unlink
        end

        def test_file_content_nonexistent_file
          assert_raises(Errno::ENOENT) do
            @builder.file_content('/nonexistent/file.txt')
          end
        end

        def test_to_h
          @builder.block(:test, 'content')
          @builder.filesystem { file 'test.txt' }

          hash = @builder.to_h

          assert_equal 1, hash[:blocks].length
          refute_nil hash[:filesystem]
          assert_equal '/', hash[:filesystem][:name]
        end

        def test_to_h_without_filesystem
          @builder.block(:test, 'content')

          hash = @builder.to_h

          assert_equal 1, hash[:blocks].length
          assert_nil hash[:filesystem]
        end

        def test_render_default
          @builder.block(:system, 'System prompt')

          output = @builder.render

          assert_includes output, '=== CONTEXT BLOCKS ==='
          assert_includes output, 'System prompt'
        end

        def test_render_json
          @builder.block(:test, 'content')

          output = @builder.render(:json)

          assert_instance_of String, output
          parsed = JSON.parse(output)
          assert_equal 1, parsed['blocks'].length
        end

        def test_render_xml
          @builder.block(:test, 'content')

          output = @builder.render(:xml)

          assert_includes output, '<context>'
          assert_includes output, '</context>'
        end

        def test_render_invalid_format
          assert_raises(Cosmos::Llm::Context::RendererNotFoundError) do
            @builder.render(:invalid_format)
          end
        end

        def test_complex_builder_chain
          builder = Builder.new do
            block :system, 'You are a helpful assistant'

            filesystem do
              directory 'src' do
                file 'main.rb', content: 'puts "Hello"'
                directory 'lib' do
                  file 'helper.rb', content: 'def help; end'
                end
              end
              file 'README.md', content: '# Project'
            end

            block :user, 'Help me with this code'
            string 'Additional context'
          end

          assert_equal 3, builder.blocks.length
          assert_equal 1, builder.root_filesystem.children.length
          assert_equal 1, builder.root_filesystem.files.length

          src_dir = builder.root_filesystem.children.first
          assert_equal 'src', src_dir.name
          assert_equal 1, src_dir.children.length
          assert_equal 1, src_dir.files.length
        end

        def test_string_returns_self_for_chaining
          result = @builder.string('content')

          assert_same @builder, result
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
