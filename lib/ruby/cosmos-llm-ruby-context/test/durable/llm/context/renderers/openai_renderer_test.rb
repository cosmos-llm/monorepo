# frozen_string_literal: true

require 'test_helper'

module Cosmos
  module Llm
    module Context
      module Renderers
        class TestOpenAiRenderer < Minitest::Test
          def setup
            @renderer = OpenAiRenderer
          end

          def test_render_simple_builder
            builder = Context.build do
              block :system, 'System prompt'
              block :user, 'User message'
            end

            output = @renderer.render(builder)
            expected = "SYSTEM:\nSystem prompt\n\nUSER:\nUser message\n"
            assert_equal expected, output
          end

          def test_render_empty_builder
            builder = Context.build
            output = @renderer.render(builder)
            assert_equal '', output
          end

          def test_render_builder_with_metadata
            builder = Context.build do
              block :system, 'Content', role: 'system', priority: 1
              block :user, 'Message', timestamp: Time.now.to_i
            end

            output = @renderer.render(builder)
            # Metadata is ignored in OpenAI renderer
            expected = "SYSTEM:\nContent\n\nUSER:\nMessage\n"
            assert_equal expected, output
          end

          def test_render_builder_with_nil_content
            builder = Context.build do
              block :test, nil
            end

            output = @renderer.render(builder)
            assert_equal '', output
          end

          def test_render_builder_with_numeric_content
            builder = Context.build do
              block :number, 42
              block :float, 3.14
            end

            output = @renderer.render(builder)
            expected = "42\n\n3.14\n"
            assert_equal expected, output
          end

          def test_render_builder_with_array_content
            builder = Context.build do
              block :list, [1, 2, 3]
              block :hash, { key: 'value' }
            end

            output = @renderer.render(builder)
            expected = "[1, 2, 3]\n\n{:key=>\"value\"}\n"
            assert_equal expected, output
          end

          def test_render_builder_with_symbol_name
            builder = Context.build do
              block :test_block, 'content'
            end

            output = @renderer.render(builder)
            assert_equal "content\n", output
          end

          def test_render_builder_with_string_name
            builder = Context.build do
              block 'custom_type', 'content'
            end

            output = @renderer.render(builder)
            assert_equal "content\n", output
          end

          def test_render_builder_multiple_blocks
            builder = Context.build do
              block :system, 'System'
              block :user, 'User'
              block :assistant, 'Assistant'
            end

            output = @renderer.render(builder)
            expected = "SYSTEM:\nSystem\n\nUSER:\nUser\n\nASSISTANT:\nAssistant\n"
            assert_equal expected, output
          end

          def test_output_is_string
            builder = Context.build do
              block :test, 'content'
            end

            output = @renderer.render(builder)
            assert_instance_of String, output
          end

          def test_render_complex_content
            content = "Complex: \n\t\"quotes\" 'single' & < >"
            builder = Context.build do
              block :complex, content
            end

            output = @renderer.render(builder)
            expected = "#{content}\n"
            assert_equal expected, output
          end

          def test_render_unicode_content
            content = 'Unicode: 你好 🌟 émojis'
            builder = Context.build do
              block :unicode, content
            end

            output = @renderer.render(builder)
            expected = "#{content}\n"
            assert_equal expected, output
          end

          def test_render_boolean_content
            builder = Context.build do
              block :bool_true, true
              block :bool_false, false
            end

            output = @renderer.render(builder)
            expected = "true\n\nfalse\n"
            assert_equal expected, output
          end

          # Test filesystem rendering
          def test_render_builder_with_filesystem_mock
            mock_fs = Minitest::Mock.new
            mock_fs.expect :tree, 'test tree'
            mock_file = Minitest::Mock.new
            mock_file.expect :content, 'content'
            mock_fs.expect :all_files, [{ file: mock_file, path: 'test.txt' }]

            builder = Context.build
            builder.instance_variable_set(:@root_filesystem, mock_fs)

            output = @renderer.render(builder)
            expected = "## Project Structure\n```\ntest tree\n```\n\n### test.txt\n```\ncontent\n```\n"
            assert_equal expected, output
          end

          def test_render_builder_only_filesystem_mock
            mock_fs = Minitest::Mock.new
            mock_fs.expect :tree, 'root tree'
            mock_fs.expect :all_files, []

            builder = Context.build
            builder.instance_variable_set(:@root_filesystem, mock_fs)

            output = @renderer.render(builder)
            expected = "## Project Structure\n```\nroot tree\n```\n"
            assert_equal expected, output
          end

          def test_render_builder_no_filesystem
            builder = Context.build do
              block :test, 'content'
            end

            output = @renderer.render(builder)
            assert_equal "content\n", output
          end

          def test_render_large_content
            large_content = 'a' * 10_000
            builder = Context.build do
              block :large, large_content
            end

            output = @renderer.render(builder)
            expected = "#{large_content}\n"
            assert_equal expected, output
          end

          def test_render_filesystem_with_empty_file
            mock_fs = Minitest::Mock.new
            mock_fs.expect :tree, 'tree'
            mock_file = Minitest::Mock.new
            mock_file.expect :content, ''
            mock_fs.expect :all_files, [{ file: mock_file, path: 'empty.txt' }]

            builder = Context.build
            builder.instance_variable_set(:@root_filesystem, mock_fs)

            output = @renderer.render(builder)
            expected = "## Project Structure\n```\ntree\n```\n"
            assert_equal expected, output
          end

          def test_render_filesystem_with_nil_content
            mock_fs = Minitest::Mock.new
            mock_fs.expect :tree, 'tree'
            mock_file = Minitest::Mock.new
            mock_file.expect :content, nil
            mock_fs.expect :all_files, [{ file: mock_file, path: 'nil.txt' }]

            builder = Context.build
            builder.instance_variable_set(:@root_filesystem, mock_fs)

            output = @renderer.render(builder)
            expected = "## Project Structure\n```\ntree\n```\n"
            assert_equal expected, output
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
