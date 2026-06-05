# frozen_string_literal: true

require 'test_helper'
require 'json'

module Cosmos
  module Llm
    module Context
      class TestRenderers < Minitest::Test
        def setup
          @builder = Builder.new do
            block :system, 'You are a helpful assistant'
            block :user, 'Hello, world!'
          end
        end

        def test_get_renderer_default
          renderer = Renderers.get_renderer(:default)

          assert_equal Renderers::DefaultRenderer, renderer
        end

        def test_get_renderer_xml
          renderer = Renderers.get_renderer(:xml)

          assert_equal Renderers::XmlRenderer, renderer
        end

        def test_get_renderer_json
          renderer = Renderers.get_renderer(:json)

          assert_equal Renderers::JsonRenderer, renderer
        end

        def test_get_renderer_anthropic
          renderer = Renderers.get_renderer(:anthropic)

          assert_equal Renderers::AnthropicRenderer, renderer
        end

        def test_get_renderer_openai
          renderer = Renderers.get_renderer(:openai)

          assert_equal Renderers::OpenAiRenderer, renderer
        end

        def test_get_renderer_unknown
          error = assert_raises(RendererNotFoundError) do
            Renderers.get_renderer(:unknown)
          end

          assert_match(/Unknown renderer format/, error.message)
        end

        def test_register_valid_renderer
          custom_renderer = Class.new do
            def self.render(_builder)
              'custom output'
            end
          end

          Renderers.register(:custom, custom_renderer)

          assert_equal custom_renderer, Renderers.get_renderer(:custom)
          assert_includes Renderers.available_formats, :custom
          assert Renderers.registered?(:custom)
        end

        def test_register_with_string_name
          custom_renderer = Class.new do
            def self.render(_builder)
              'string name output'
            end
          end

          Renderers.register('string_custom', custom_renderer)

          assert_equal custom_renderer, Renderers.get_renderer(:string_custom)
          assert Renderers.registered?(:string_custom)
        end

        def test_register_invalid_renderer_no_render_method
          invalid_renderer = Class.new

          error = assert_raises(ArgumentError) do
            Renderers.register(:invalid, invalid_renderer)
          end

          assert_match(/Renderer class must respond to \.render method/, error.message)
        end

        def test_register_duplicate_without_overwrite
          first_renderer = Class.new do
            def self.render(_builder)
              'first'
            end
          end

          second_renderer = Class.new do
            def self.render(_builder)
              'second'
            end
          end

          Renderers.register(:duplicate_test, first_renderer)

          error = assert_raises(DuplicateRegistrationError) do
            Renderers.register(:duplicate_test, second_renderer)
          end

          assert_match(/Renderer 'duplicate_test' is already registered/, error.message)
          assert_equal first_renderer, Renderers.get_renderer(:duplicate_test)
        end

        def test_register_duplicate_with_overwrite
          first_renderer = Class.new do
            def self.render(_builder)
              'first'
            end
          end

          second_renderer = Class.new do
            def self.render(_builder)
              'second'
            end
          end

          Renderers.register(:overwrite_test, first_renderer)
          Renderers.register(:overwrite_test, second_renderer, overwrite: true)

          assert_equal second_renderer, Renderers.get_renderer(:overwrite_test)
        end

        def test_available_formats_includes_builtin_and_registered
          initial_formats = Renderers.available_formats
          assert_includes initial_formats, :default
          assert_includes initial_formats, :xml
          assert_includes initial_formats, :json
          assert_includes initial_formats, :anthropic
          assert_includes initial_formats, :openai

          custom_renderer = Class.new do
            def self.render(_builder)
              'test'
            end
          end

          Renderers.register(:test_format, custom_renderer)
          updated_formats = Renderers.available_formats

          assert_includes updated_formats, :test_format
          assert_equal initial_formats.length + 1, updated_formats.length
        end

        def test_registered_returns_true_for_builtin
          assert Renderers.registered?(:default)
          assert Renderers.registered?(:xml)
          assert Renderers.registered?(:json)
          assert Renderers.registered?(:anthropic)
          assert Renderers.registered?(:openai)
        end

        def test_registered_returns_false_for_unknown
          refute Renderers.registered?(:unknown_format)
        end

        def test_registered_with_string_name
          assert Renderers.registered?('default')
          refute Renderers.registered?('unknown')
        end

        def test_get_renderer_custom_registered
          custom_renderer = Class.new do
            def self.render(builder)
              "custom rendered: #{builder.blocks.length} blocks"
            end
          end

          Renderers.register(:custom_registered, custom_renderer)

          builder = Builder.new do
            block :test, 'content'
          end

          output = Renderers.get_renderer(:custom_registered).render(builder)
          assert_equal 'custom rendered: 1 blocks', output
        end
      end

      class TestDefaultRenderer < Minitest::Test
        def test_render_with_blocks_only
          builder = Builder.new do
            block :system, 'System message'
            block :user, 'User message'
          end

          output = Renderers::DefaultRenderer.render(builder)

          assert_includes output, '=== CONTEXT BLOCKS ==='
          assert_includes output, '[SYSTEM]'
          assert_includes output, 'System message'
          assert_includes output, '[USER]'
          assert_includes output, 'User message'
        end

        def test_render_with_filesystem_only
          builder = Builder.new do
            filesystem do
              file 'test.txt', content: 'content'
            end
          end

          output = Renderers::DefaultRenderer.render(builder)

          assert_includes output, '=== FILESYSTEM ==='
          assert_includes output, '/'
          assert_includes output, 'test.txt'
        end

        def test_render_with_blocks_and_filesystem
          builder = Builder.new do
            block :system, 'System'
            filesystem { file 'test.txt' }
          end

          output = Renderers::DefaultRenderer.render(builder)

          assert_includes output, '=== FILESYSTEM ==='
          assert_includes output, '=== CONTEXT BLOCKS ==='
        end

        def test_render_empty_builder
          builder = Builder.new

          output = Renderers::DefaultRenderer.render(builder)

          refute_includes output, '=== CONTEXT BLOCKS ==='
          refute_includes output, '=== FILESYSTEM ==='
        end

        def test_render_block_with_nil_content
          builder = Builder.new do
            block :test, nil
          end

          output = Renderers::DefaultRenderer.render(builder)

          assert_includes output, '[TEST]'
          # nil.to_s is empty string, so output should end with [TEST] followed by newline
          assert output.end_with?("[TEST]\n")
        end

        def test_render_block_with_empty_content
          builder = Builder.new do
            block :test, ''
          end

          output = Renderers::DefaultRenderer.render(builder)

          assert_includes output, '[TEST]'
          assert_includes output, ''
        end

        def test_render_block_with_numeric_content
          builder = Builder.new do
            block :count, 42
          end

          output = Renderers::DefaultRenderer.render(builder)

          assert_includes output, '[COUNT]'
          assert_includes output, '42'
        end

        def test_render_block_with_multiline_content
          multiline_content = "Line 1\nLine 2\nLine 3"
          builder = Builder.new do
            block :multiline, multiline_content
          end

          output = Renderers::DefaultRenderer.render(builder)

          assert_includes output, '[MULTILINE]'
          assert_includes output, multiline_content
        end

        def test_render_block_with_string_name
          builder = Builder.new do
            block 'custom', 'content'
          end

          output = Renderers::DefaultRenderer.render(builder)

          assert_includes output, '[CUSTOM]'
          assert_includes output, 'content'
        end

        def test_render_block_with_symbol_name
          builder = Builder.new do
            block :symbol_name, 'content'
          end

          output = Renderers::DefaultRenderer.render(builder)

          assert_includes output, '[SYMBOL_NAME]'
          assert_includes output, 'content'
        end

        def test_render_block_with_special_characters_in_name
          builder = Builder.new do
            block :test_name, 'content'
          end

          output = Renderers::DefaultRenderer.render(builder)

          assert_includes output, '[TEST_NAME]'
          assert_includes output, 'content'
        end

        def test_render_multiple_blocks_order
          builder = Builder.new do
            block :first, 'First content'
            block :second, 'Second content'
            block :third, 'Third content'
          end

          output = Renderers::DefaultRenderer.render(builder)

          lines = output.split("\n")
          first_index = lines.index { |l| l == '[FIRST]' }
          second_index = lines.index { |l| l == '[SECOND]' }
          third_index = lines.index { |l| l == '[THIRD]' }

          assert first_index < second_index
          assert second_index < third_index
        end

        def test_render_filesystem_with_nested_directories
          builder = Builder.new do
            filesystem do
              directory 'src' do
                directory 'lib' do
                  file 'main.rb', content: 'code'
                end
                file 'readme.txt', content: 'info'
              end
              file 'root.txt', content: 'root'
            end
          end

          output = Renderers::DefaultRenderer.render(builder)

          assert_includes output, '=== FILESYSTEM ==='
          assert_includes output, '/'
          assert_includes output, 'root.txt'
          assert_includes output, 'src'
          assert_includes output, 'lib'
          assert_includes output, 'main.rb'
          assert_includes output, 'readme.txt'
        end

        def test_render_filesystem_with_empty_file
          builder = Builder.new do
            filesystem do
              file 'empty.txt'
            end
          end

          output = Renderers::DefaultRenderer.render(builder)

          assert_includes output, '=== FILESYSTEM ==='
          assert_includes output, 'empty.txt'
        end

        def test_render_complex_builder
          builder = Builder.new do
            filesystem do
              directory 'app' do
                file 'main.rb', content: 'puts "hello"'
              end
            end
            block :system, 'You are a helpful assistant'
            block :user, 'Write a Ruby script'
            block :assistant, 'Here is the script:'
          end

          output = Renderers::DefaultRenderer.render(builder)

          # Filesystem section first
          filesystem_section_start = output.index('=== FILESYSTEM ===')
          blocks_section_start = output.index('=== CONTEXT BLOCKS ===')

          assert filesystem_section_start < blocks_section_start

          # Check filesystem content
          assert_includes output, 'app'
          assert_includes output, 'main.rb'

          # Check blocks
          assert_includes output, '[SYSTEM]'
          assert_includes output, '[USER]'
          assert_includes output, '[ASSISTANT]'
          assert_includes output, 'You are a helpful assistant'
          assert_includes output, 'Write a Ruby script'
          assert_includes output, 'Here is the script:'
        end

        def test_render_output_format_exact
          builder = Builder.new do
            block :test, 'content'
          end

          output = Renderers::DefaultRenderer.render(builder)

          expected = "=== CONTEXT BLOCKS ===\n\n[TEST]\ncontent"
          assert_equal expected, output
        end

        def test_render_filesystem_and_blocks_format
          builder = Builder.new do
            filesystem do
              file 'test.txt', content: 'file content'
            end
            block :test, 'block content'
          end

          output = Renderers::DefaultRenderer.render(builder)

          lines = output.split("\n")
          assert_equal '=== FILESYSTEM ===', lines[0]
          # Filesystem tree content would be on subsequent lines
          blocks_index = lines.index('=== CONTEXT BLOCKS ===')
          assert blocks_index.positive?
          # After blocks header, there should be a blank line then [TEST]
          assert_equal '', lines[blocks_index + 1]
          assert_equal '[TEST]', lines[blocks_index + 2]
          assert_equal 'block content', lines[blocks_index + 3]
        end

        def test_render_class_method
          assert_respond_to Renderers::DefaultRenderer, :render
          assert_equal '=== CONTEXT BLOCKS ===', Renderers::DefaultRenderer.render(Builder.new {
            block :test, ''
          }).split("\n").first
        end
      end

      class TestXmlRenderer < Minitest::Test
        def test_render_basic_structure
          builder = Builder.new do
            block :test, 'content'
          end

          output = Renderers::XmlRenderer.render(builder)

          assert_includes output, '<context>'
          assert_includes output, '</context>'
        end

        def test_render_block
          builder = Builder.new do
            block :system, 'System message'
          end

          output = Renderers::XmlRenderer.render(builder)

          assert_includes output, '<block type="system">'
          assert_includes output, 'System message'
          assert_includes output, '</block>'
        end

        def test_render_multiple_blocks
          builder = Builder.new do
            block :system, 'System'
            block :user, 'User'
          end

          output = Renderers::XmlRenderer.render(builder)

          assert_includes output, '<block type="system">'
          assert_includes output, '<block type="user">'
        end

        def test_render_filesystem
          builder = Builder.new do
            filesystem do
              file 'test.txt', content: 'Hello'
            end
          end

          output = Renderers::XmlRenderer.render(builder)

          assert_includes output, '<filesystem name="/">'
          assert_includes output, '<file name="test.txt">'
          assert_includes output, 'Hello'
          assert_includes output, '</file>'
          assert_includes output, '</filesystem>'
        end

        def test_render_nested_filesystem
          builder = Builder.new do
            filesystem do
              directory 'src' do
                file 'main.rb', content: 'code'
              end
            end
          end

          output = Renderers::XmlRenderer.render(builder)

          assert_includes output, '<filesystem name="/">'
          assert_includes output, '<filesystem name="src">'
          assert_includes output, '<file name="main.rb">'
        end

        def test_escape_xml_special_characters
          escaped = Renderers::XmlRenderer.escape_xml('<>&"\'')

          assert_equal '&lt;&gt;&amp;&quot;&apos;', escaped
        end

        def test_render_with_special_characters
          builder = Builder.new do
            block :test, 'Content with <tags> & "quotes"'
          end

          output = Renderers::XmlRenderer.render(builder)

          assert_includes output, '&lt;tags&gt;'
          assert_includes output, '&amp;'
          assert_includes output, '&quot;quotes&quot;'
        end
      end

      class TestJsonRenderer < Minitest::Test
        def test_render_returns_valid_json
          builder = Builder.new do
            block :test, 'content'
          end

          output = Renderers::JsonRenderer.render(builder)
          parsed = JSON.parse(output)

          assert_instance_of Hash, parsed
        end

        def test_render_includes_blocks
          builder = Builder.new do
            block :system, 'System message'
            block :user, 'User message'
          end

          output = Renderers::JsonRenderer.render(builder)
          parsed = JSON.parse(output)

          assert_equal 2, parsed['blocks'].length
          assert_equal 'system', parsed['blocks'][0]['name']
          assert_equal 'user', parsed['blocks'][1]['name']
        end

        def test_render_includes_filesystem
          builder = Builder.new do
            filesystem do
              file 'test.txt', content: 'content'
            end
          end

          output = Renderers::JsonRenderer.render(builder)
          parsed = JSON.parse(output)

          refute_nil parsed['filesystem']
          assert_equal '/', parsed['filesystem']['name']
          assert_equal 1, parsed['filesystem']['files'].length
        end

        def test_render_nil_filesystem
          builder = Builder.new do
            block :test, 'content'
          end

          output = Renderers::JsonRenderer.render(builder)
          parsed = JSON.parse(output)

          assert_nil parsed['filesystem']
        end
      end

      class TestAnthropicRenderer < Minitest::Test
        def test_render_system_block_first
          builder = Builder.new do
            block :user, 'User message'
            block :system, 'System prompt'
          end

          output = Renderers::AnthropicRenderer.render(builder)
          lines = output.split("\n")

          # System prompt should come before user message
          system_index = lines.index { |l| l.include?('System prompt') }
          user_index = lines.index { |l| l.include?('<user>') }

          assert system_index < user_index
        end

        def test_render_filesystem_in_xml_tags
          builder = Builder.new do
            filesystem do
              file 'test.txt', content: 'Hello'
            end
          end

          output = Renderers::AnthropicRenderer.render(builder)

          assert_includes output, '<filesystem>'
          assert_includes output, '</filesystem>'
          assert_includes output, '<file path="/test.txt">'
          assert_includes output, 'Hello'
          assert_includes output, '</file>'
        end

        def test_render_blocks_with_xml_tags
          builder = Builder.new do
            block :user, 'User message'
            block :context, 'Additional context'
          end

          output = Renderers::AnthropicRenderer.render(builder)

          assert_includes output, '<user>'
          assert_includes output, '</user>'
          assert_includes output, '<context>'
          assert_includes output, '</context>'
        end

        def test_render_empty_file
          builder = Builder.new do
            filesystem do
              file 'empty.txt'
            end
          end

          output = Renderers::AnthropicRenderer.render(builder)

          assert_includes output, '<file path="/empty.txt" />'
        end

        def test_render_nested_filesystem_paths
          builder = Builder.new do
            filesystem do
              directory 'src' do
                directory 'lib' do
                  file 'helper.rb', content: 'code'
                end
              end
            end
          end

          output = Renderers::AnthropicRenderer.render(builder)

          assert_includes output, '<file path="/src/lib/helper.rb">'
        end

        def test_render_no_system_blocks
          builder = Builder.new do
            block :user, 'User message'
            block :assistant, 'Assistant response'
          end

          output = Renderers::AnthropicRenderer.render(builder)

          # Should start directly with user block
          lines = output.split("\n")
          refute lines.first.include?('System')
          assert_includes output, '<user>'
          assert_includes output, '<assistant>'
        end

        def test_render_only_system_blocks
          builder = Builder.new do
            block :system, 'System prompt 1'
            block :system, 'System prompt 2'
          end

          output = Renderers::AnthropicRenderer.render(builder)

          # Should contain system content directly, no XML tags for system
          assert_includes output, 'System prompt 1'
          assert_includes output, 'System prompt 2'
          refute_includes output, '<system>'
        end

        def test_render_no_filesystem
          builder = Builder.new do
            block :user, 'Message'
          end

          output = Renderers::AnthropicRenderer.render(builder)

          refute_includes output, '<filesystem>'
          refute_includes output, '</filesystem>'
        end

        def test_render_empty_filesystem
          builder = Builder.new do
            filesystem do
              # Empty filesystem
            end
            block :user, 'Message'
          end

          output = Renderers::AnthropicRenderer.render(builder)

          assert_includes output, '<filesystem>'
          assert_includes output, '</filesystem>'
          # Should still have the user block after
          assert_includes output, '<user>'
        end

        def test_render_multiple_system_blocks_order
          builder = Builder.new do
            block :user, 'User'
            block :system, 'System 1'
            block :system, 'System 2'
            block :assistant, 'Assistant'
          end

          output = Renderers::AnthropicRenderer.render(builder)
          lines = output.split("\n")

          # Find indices
          system1_index = lines.index { |l| l.include?('System 1') }
          system2_index = lines.index { |l| l.include?('System 2') }
          user_index = lines.index { |l| l.include?('<user>') }
          assistant_index = lines.index { |l| l.include?('<assistant>') }

          # System blocks should be first, in order
          assert system1_index < system2_index
          assert system2_index < user_index
          assert user_index < assistant_index
        end

        def test_render_block_with_nil_content
          builder = Builder.new do
            block :test, nil
          end

          output = Renderers::AnthropicRenderer.render(builder)

          assert_includes output, '<test>'
          assert_includes output, '</test>'
          # nil.to_s is empty string, so there should be an empty line between tags
          lines = output.split("\n")
          test_start = lines.index('<test>')
          test_end = lines.index('</test>')
          assert_equal test_start + 2, test_end # <test>, empty line, </test>
          assert_equal '', lines[test_start + 1]
        end

        def test_render_block_with_empty_content
          builder = Builder.new do
            block :test, ''
          end

          output = Renderers::AnthropicRenderer.render(builder)

          assert_includes output, '<test>'
          assert_includes output, '</test>'
          lines = output.split("\n")
          test_start = lines.index('<test>')
          test_end = lines.index('</test>')
          assert_equal test_start + 2, test_end # <test>, empty line, </test>
          assert_equal '', lines[test_start + 1]
        end

        def test_render_block_with_multiline_content
          multiline = "Line 1\nLine 2\nLine 3"
          builder = Builder.new do
            block :test, multiline
          end

          output = Renderers::AnthropicRenderer.render(builder)

          assert_includes output, '<test>'
          assert_includes output, 'Line 1'
          assert_includes output, 'Line 2'
          assert_includes output, 'Line 3'
          assert_includes output, '</test>'
        end

        def test_render_block_with_special_characters_in_name
          builder = Builder.new do
            block :test_name, 'content'
          end

          output = Renderers::AnthropicRenderer.render(builder)

          assert_includes output, '<test_name>'
          assert_includes output, '</test_name>'
        end

        def test_render_file_with_special_characters_in_name
          builder = Builder.new do
            filesystem do
              file 'test-file_name.rb', content: 'code'
            end
          end

          output = Renderers::AnthropicRenderer.render(builder)

          assert_includes output, '<file path="/test-file_name.rb">'
        end

        def test_render_deeply_nested_filesystem
          builder = Builder.new do
            filesystem do
              directory 'level1' do
                directory 'level2' do
                  directory 'level3' do
                    file 'deep.txt', content: 'deep content'
                  end
                end
              end
            end
          end

          output = Renderers::AnthropicRenderer.render(builder)

          assert_includes output, '<file path="/level1/level2/level3/deep.txt">'
          assert_includes output, 'deep content'
        end

        def test_render_filesystem_position_between_system_and_blocks
          builder = Builder.new do
            block :system, 'System'
            filesystem do
              file 'test.txt', content: 'content'
            end
            block :user, 'User'
          end

          output = Renderers::AnthropicRenderer.render(builder)
          lines = output.split("\n")

          system_index = lines.index { |l| l.include?('System') }
          filesystem_start = lines.index('<filesystem>')
          user_start = lines.index('<user>')

          assert system_index < filesystem_start
          assert filesystem_start < user_start
        end

        def test_render_output_format_exact_simple
          builder = Builder.new do
            block :user, 'Hello'
          end

          output = Renderers::AnthropicRenderer.render(builder)

          expected = "\n<user>\nHello\n</user>"
          assert_equal expected, output
        end

        def test_render_output_format_exact_with_filesystem
          builder = Builder.new do
            filesystem do
              file 'test.txt', content: 'content'
            end
            block :user, 'Hello'
          end

          output = Renderers::AnthropicRenderer.render(builder)

          expected = "\n<filesystem>\n\n<file path=\"/test.txt\">\ncontent\n</file>\n</filesystem>\n\n<user>\nHello\n" \
                     '</user>'
          assert_equal expected, output
        end

        def test_render_filesystem_with_non_root_name
          # This tests the path logic for non-standard filesystem names
          builder = Builder.new do
            filesystem do
              # Assuming filesystem can have custom name, but in practice it's usually '/'
              # This tests the code path where fs.name != '/'
            end
          end

          output = Renderers::AnthropicRenderer.render(builder)

          # For root '/', it should be <filesystem> without name attribute
          assert_includes output, '<filesystem>'
          assert_includes output, '</filesystem>'
        end

        def test_render_complex_scenario
          builder = Builder.new do
            block :system, 'You are an AI assistant.'
            filesystem do
              directory 'src' do
                file 'main.rb', content: 'puts "hello"'
                file 'empty.rb'
              end
              file 'readme.md', content: '# README'
            end
            block :user, 'What does this code do?'
            block :assistant, 'It prints hello.'
          end

          output = Renderers::AnthropicRenderer.render(builder)

          # Check order: system first, then filesystem, then other blocks
          lines = output.split("\n")
          system_index = lines.index { |l| l.include?('You are an AI assistant.') }
          filesystem_index = lines.index('<filesystem>')
          user_index = lines.index('<user>')
          assistant_index = lines.index('<assistant>')

          assert system_index < filesystem_index
          assert filesystem_index < user_index
          assert user_index < assistant_index

          # Check content
          assert_includes output, 'puts "hello"'
          assert_includes output, '# README'
          assert_includes output, '<file path="/readme.md">'
          assert_includes output, '<file path="/src/main.rb">'
          assert_includes output, '<file path="/src/empty.rb" />'
        end
      end

      class TestOpenAiRenderer < Minitest::Test
        def test_render_system_block
          builder = Builder.new do
            block :system, 'You are helpful'
          end

          output = Renderers::OpenAiRenderer.render(builder)

          assert_includes output, 'SYSTEM:'
          assert_includes output, 'You are helpful'
        end

        def test_render_user_block
          builder = Builder.new do
            block :user, 'Hello'
          end

          output = Renderers::OpenAiRenderer.render(builder)

          assert_includes output, 'USER:'
          assert_includes output, 'Hello'
        end

        def test_render_assistant_block
          builder = Builder.new do
            block :assistant, 'Hi there'
          end

          output = Renderers::OpenAiRenderer.render(builder)

          assert_includes output, 'ASSISTANT:'
          assert_includes output, 'Hi there'
        end

        def test_render_filesystem_as_markdown
          builder = Builder.new do
            filesystem do
              file 'test.txt', content: 'Hello'
            end
          end

          output = Renderers::OpenAiRenderer.render(builder)

          assert_includes output, '## Project Structure'
          assert_includes output, '```'
          assert_includes output, 'test.txt'
        end

        def test_render_file_contents_as_code_blocks
          builder = Builder.new do
            filesystem do
              directory 'src' do
                file 'main.rb', content: 'puts "Hello"'
              end
            end
          end

          output = Renderers::OpenAiRenderer.render(builder)

          assert_includes output, '### /src/main.rb'
          assert_includes output, '```'
          assert_includes output, 'puts "Hello"'
        end

        def test_render_skips_empty_files
          builder = Builder.new do
            filesystem do
              file 'empty.txt'
              file 'nonempty.txt', content: 'content'
            end
          end

          output = Renderers::OpenAiRenderer.render(builder)

          refute_includes output, '### /empty.txt'
          assert_includes output, '### /nonempty.txt'
        end

        def test_render_other_block_types
          builder = Builder.new do
            block :custom, 'Custom content'
          end

          output = Renderers::OpenAiRenderer.render(builder)

          assert_includes output, 'Custom content'
          refute_includes output, 'CUSTOM:' # Should not add label for non-standard types
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
