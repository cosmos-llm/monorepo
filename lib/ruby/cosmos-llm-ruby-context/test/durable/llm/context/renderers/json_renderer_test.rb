# frozen_string_literal: true

require 'test_helper'

module Cosmos
  module Llm
    module Context
      module Renderers
        class TestJsonRenderer < Minitest::Test
          def setup
            @renderer = JsonRenderer
          end

          def test_render_simple_builder
            builder = Context.build do
              block :system, 'System prompt'
              block :user, 'User message'
            end

            output = @renderer.render(builder)
            parsed = JSON.parse(output)

            assert_equal 2, parsed['blocks'].length
            assert_equal 'system', parsed['blocks'][0]['name']
            assert_equal 'System prompt', parsed['blocks'][0]['content']
            assert_equal({}, parsed['blocks'][0]['metadata'])
            assert_equal 'user', parsed['blocks'][1]['name']
            assert_equal 'User message', parsed['blocks'][1]['content']
            assert_nil parsed['filesystem']
          end

          def test_render_empty_builder
            builder = Context.build
            output = @renderer.render(builder)
            parsed = JSON.parse(output)

            assert_equal([], parsed['blocks'])
            assert_nil parsed['filesystem']
          end

          def test_render_builder_with_metadata
            builder = Context.build do
              block :system, 'Content', role: 'system', priority: 1
              block :user, 'Message', timestamp: Time.now.to_i
            end

            output = @renderer.render(builder)
            parsed = JSON.parse(output)

            assert_equal 2, parsed['blocks'].length
            assert_equal({ 'role' => 'system', 'priority' => 1 }, parsed['blocks'][0]['metadata'])
            assert_equal 'Message', parsed['blocks'][1]['content']
            assert parsed['blocks'][1]['metadata'].key?('timestamp')
          end

          def test_render_builder_with_nil_content
            builder = Context.build do
              block :test, nil
            end

            output = @renderer.render(builder)
            parsed = JSON.parse(output)

            assert_equal 1, parsed['blocks'].length
            assert_nil parsed['blocks'][0]['content']
          end

          def test_render_builder_with_numeric_content
            builder = Context.build do
              block :number, 42
              block :float, 3.14
            end

            output = @renderer.render(builder)
            parsed = JSON.parse(output)

            assert_equal 42, parsed['blocks'][0]['content']
            assert_equal 3.14, parsed['blocks'][1]['content']
          end

          def test_render_builder_with_array_content
            builder = Context.build do
              block :list, [1, 2, 3]
              block :hash, { key: 'value' }
            end

            output = @renderer.render(builder)
            parsed = JSON.parse(output)

            assert_equal [1, 2, 3], parsed['blocks'][0]['content']
            assert_equal({ 'key' => 'value' }, parsed['blocks'][1]['content'])
          end

          def test_render_builder_with_symbol_name
            builder = Context.build do
              block :test_block, 'content'
            end

            output = @renderer.render(builder)
            parsed = JSON.parse(output)

            assert_equal 'test_block', parsed['blocks'][0]['name']
          end

          def test_render_builder_with_string_name
            builder = Context.build do
              block 'custom_type', 'content'
            end

            output = @renderer.render(builder)
            parsed = JSON.parse(output)

            assert_equal 'custom_type', parsed['blocks'][0]['name']
          end

          def test_render_builder_multiple_blocks
            builder = Context.build do
              block :system, 'System'
              block :user, 'User'
              block :assistant, 'Assistant'
            end

            output = @renderer.render(builder)
            parsed = JSON.parse(output)

            assert_equal 3, parsed['blocks'].length
            names = parsed['blocks'].map { |b| b['name'] }
            contents = parsed['blocks'].map { |b| b['content'] }
            assert_equal %w[system user assistant], names
            assert_equal %w[System User Assistant], contents
          end

          def test_output_is_valid_json
            builder = Context.build do
              block :test, 'content'
            end

            output = @renderer.render(builder)
            assert_instance_of String, output

            # Should not raise an exception
            parsed = JSON.parse(output)
            assert parsed.is_a?(Hash)
          end

          def test_output_is_pretty_printed
            builder = Context.build do
              block :test, 'content'
            end

            output = @renderer.render(builder)
            lines = output.split("\n")

            # Pretty-printed JSON should have multiple lines
            assert lines.length > 1
            # Should have indentation
            assert(lines.any? { |line| line.start_with?('  ') })
          end

          def test_render_complex_metadata
            metadata = {
              nested: { key: 'value' },
              array: [1, 2, 'three'],
              boolean: true,
              null: nil
            }

            builder = Context.build do
              block :complex, 'content', metadata
            end

            output = @renderer.render(builder)
            parsed = JSON.parse(output)

            assert_equal metadata, parsed['blocks'][0]['metadata']
          end

          def test_render_empty_metadata
            builder = Context.build do
              block :no_meta, 'content'
            end

            output = @renderer.render(builder)
            parsed = JSON.parse(output)

            assert_equal({}, parsed['blocks'][0]['metadata'])
          end

          def test_render_large_content
            large_content = 'a' * 10_000
            builder = Context.build do
              block :large, large_content
            end

            output = @renderer.render(builder)
            parsed = JSON.parse(output)

            assert_equal large_content, parsed['blocks'][0]['content']
          end

          def test_render_special_characters
            content = "Special chars: \n\t\"quotes\" 'single' & < >"
            builder = Context.build do
              block :special, content
            end

            output = @renderer.render(builder)
            parsed = JSON.parse(output)

            assert_equal content, parsed['blocks'][0]['content']
          end

          def test_render_unicode_content
            content = 'Unicode: 你好 🌟 émojis'
            builder = Context.build do
              block :unicode, content
            end

            output = @renderer.render(builder)
            parsed = JSON.parse(output)

            assert_equal content, parsed['blocks'][0]['content']
          end

          def test_render_boolean_content
            builder = Context.build do
              block :bool_true, true
              block :bool_false, false
            end

            output = @renderer.render(builder)
            parsed = JSON.parse(output)

            assert_equal true, parsed['blocks'][0]['content']
            assert_equal false, parsed['blocks'][1]['content']
          end

          # Test filesystem rendering (mocked since Filesystem is from another gem)
          def test_render_builder_with_filesystem_mock
            # Mock a filesystem object
            mock_fs = Minitest::Mock.new
            mock_fs.expect :to_h, {
              name: '/',
              files: [{ name: 'test.txt', content: 'content' }],
              children: []
            }

            builder = Context.build
            builder.instance_variable_set(:@root_filesystem, mock_fs)

            output = @renderer.render(builder)
            parsed = JSON.parse(output)

            assert_equal({
                           'name' => '/',
                           'files' => [{ 'name' => 'test.txt', 'content' => 'content' }],
                           'children' => []
                         }, parsed['filesystem'])
          end

          def test_render_builder_only_filesystem_mock
            mock_fs = Minitest::Mock.new
            mock_fs.expect :to_h, { name: 'root', files: [], children: [] }

            builder = Context.build
            builder.instance_variable_set(:@root_filesystem, mock_fs)

            output = @renderer.render(builder)
            parsed = JSON.parse(output)

            assert_equal([], parsed['blocks'])
            assert_equal({ 'name' => 'root', 'files' => [], 'children' => [] }, parsed['filesystem'])
          end

          def test_render_builder_no_filesystem
            builder = Context.build do
              block :test, 'content'
            end

            output = @renderer.render(builder)
            parsed = JSON.parse(output)

            assert_nil parsed['filesystem']
          end

          # Error handling tests
          def test_render_with_non_serializable_content
            # Create a block with non-serializable content
            builder = Context.build
            # Manually create a block with Time object (not JSON serializable by default)
            time_block = Block.new(:time, Time.now)
            builder.blocks << time_block

            # This should raise JSON::GeneratorError
            assert_raises JSON::GeneratorError do
              @renderer.render(builder)
            end
          end

          def test_render_with_circular_reference
            # Create circular reference
            hash = {}
            hash['self'] = hash

            builder = Context.build do
              block :circular, hash
            end

            # Should raise JSON::GeneratorError for circular reference
            assert_raises JSON::GeneratorError do
              @renderer.render(builder)
            end
          end

          def test_render_with_infinite_value
            builder = Context.build do
              block :infinity, Float::INFINITY
            end

            # Infinity should be serialized as null or raise error
            # Actually, JSON.generate converts Infinity to null
            output = @renderer.render(builder)
            parsed = JSON.parse(output)

            assert_nil parsed['blocks'][0]['content']
          end

          def test_render_with_nan
            builder = Context.build do
              block :nan, Float::NAN
            end

            # NaN should be serialized as null
            output = @renderer.render(builder)
            parsed = JSON.parse(output)

            assert_nil parsed['blocks'][0]['content']
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.</content>
