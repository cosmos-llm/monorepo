# frozen_string_literal: true

require 'test_helper'

module Cosmos
  module Llm
    module Context
      module Renderers
        class TestXmlRenderer < Minitest::Test
          def setup
            @renderer = XmlRenderer
          end

          def test_escape_xml_ampersand
            assert_equal '&amp;', @renderer.escape_xml('&')
          end

          def test_escape_xml_less_than
            assert_equal '&lt;', @renderer.escape_xml('<')
          end

          def test_escape_xml_greater_than
            assert_equal '&gt;', @renderer.escape_xml('>')
          end

          def test_escape_xml_double_quote
            assert_equal '&quot;', @renderer.escape_xml('"')
          end

          def test_escape_xml_single_quote
            assert_equal '&apos;', @renderer.escape_xml("'")
          end

          def test_escape_xml_mixed_special_chars
            input = 'Tom & Jerry <friends> "best" \'duo\''
            expected = 'Tom &amp; Jerry &lt;friends&gt; &quot;best&quot; &apos;duo&apos;'
            assert_equal expected, @renderer.escape_xml(input)
          end

          def test_escape_xml_no_special_chars
            input = 'Hello World 123'
            assert_equal input, @renderer.escape_xml(input)
          end

          def test_escape_xml_empty_string
            assert_equal '', @renderer.escape_xml('')
          end

          def test_escape_xml_nil
            assert_equal '', @renderer.escape_xml(nil)
          end

          def test_escape_xml_numeric
            assert_equal '42', @renderer.escape_xml(42)
          end

          def test_escape_xml_symbol
            assert_equal 'test', @renderer.escape_xml(:test)
          end

          def test_render_block_simple
            block = Block.new(:system, 'Hello world')
            output = @renderer.render_block(block)
            expected = "  <block type=\"system\">\n    Hello world\n  </block>"
            assert_equal expected, output
          end

          def test_render_block_with_special_chars
            block = Block.new(:user, 'Test & <content>')
            output = @renderer.render_block(block)
            expected = "  <block type=\"user\">\n    Test &amp; &lt;content&gt;\n  </block>"
            assert_equal expected, output
          end

          def test_render_block_empty_content
            block = Block.new(:empty, '')
            output = @renderer.render_block(block)
            expected = "  <block type=\"empty\">\n    \n  </block>"
            assert_equal expected, output
          end

          def test_render_block_numeric_content
            block = Block.new(:number, 42)
            output = @renderer.render_block(block)
            expected = "  <block type=\"number\">\n    42\n  </block>"
            assert_equal expected, output
          end

          def test_render_block_symbol_name
            block = Block.new(:test_block, 'content')
            output = @renderer.render_block(block)
            expected = "  <block type=\"test_block\">\n    content\n  </block>"
            assert_equal expected, output
          end

          def test_render_filesystem_empty
            fs = Filesystem.new('empty')
            output = @renderer.render_filesystem(fs)
            expected = "  <filesystem name=\"empty\">\n  </filesystem>"
            assert_equal expected, output
          end

          def test_render_filesystem_with_files
            fs = Filesystem.new('project')
            fs.file('README.md', content: '# Project')
            fs.file('main.rb', content: 'puts "hello"')
            fs.file('empty.txt') # no content

            output = @renderer.render_filesystem(fs)
            lines = output.split("\n")

            assert_equal '  <filesystem name="project">', lines[0]
            assert_equal '    <file name="README.md">', lines[1]
            assert_equal '      # Project', lines[2]
            assert_equal '    </file>', lines[3]
            assert_equal '    <file name="main.rb">', lines[4]
            assert_equal '      puts &quot;hello&quot;', lines[5]
            assert_equal '    </file>', lines[6]
            assert_equal '    <file name="empty.txt">', lines[7]
            assert_equal '    </file>', lines[8]
            assert_equal '  </filesystem>', lines[9]
          end

          def test_render_filesystem_with_special_chars
            fs = Filesystem.new('test & <dir>')
            fs.file('file "name".txt', content: 'Content with \'quotes\'')

            output = @renderer.render_filesystem(fs)
            assert_includes output, '<filesystem name="test &amp; &lt;dir&gt;">'
            assert_includes output, '<file name="file &quot;name&quot;.txt">'
            assert_includes output, 'Content with &apos;quotes&apos;'
          end

          def test_render_filesystem_nested
            fs = Filesystem.new('root')
            src = fs.directory('src')
            src.file('main.rb', content: 'code')
            lib = src.directory('lib')
            lib.file('helper.rb', content: 'helper')

            output = @renderer.render_filesystem(fs)
            lines = output.split("\n")

            # Root filesystem
            assert_equal '  <filesystem name="root">', lines[0]
            # src directory
            assert_equal '    <filesystem name="src">', lines[1]
            # main.rb file
            assert_equal '      <file name="main.rb">', lines[2]
            assert_equal '        code', lines[3]
            assert_equal '      </file>', lines[4]
            # lib directory
            assert_equal '      <filesystem name="lib">', lines[5]
            # helper.rb file
            assert_equal '        <file name="helper.rb">', lines[6]
            assert_equal '          helper', lines[7]
            assert_equal '        </file>', lines[8]
            # Close lib
            assert_equal '      </filesystem>', lines[9]
            # Close src
            assert_equal '    </filesystem>', lines[10]
            # Close root
            assert_equal '  </filesystem>', lines[11]
          end

          def test_render_filesystem_deeply_nested
            fs = Filesystem.new('root')
            level1 = fs.directory('level1')
            level2 = level1.directory('level2')
            level3 = level2.directory('level3')
            level3.file('deep.txt', content: 'deep content')

            output = @renderer.render_filesystem(fs)
            # Check indentation increases with depth
            assert_includes output, '  <filesystem name="root">'
            assert_includes output, '    <filesystem name="level1">'
            assert_includes output, '      <filesystem name="level2">'
            assert_includes output, '        <filesystem name="level3">'
            assert_includes output, '          <file name="deep.txt">'
            assert_includes output, '            deep content'
          end

          def test_render_simple_builder
            builder = Context.build do
              block :system, 'System prompt'
              block :user, 'User message'
            end

            output = @renderer.render(builder)
            lines = output.split("\n")

            assert_equal '<context>', lines[0]
            assert_equal '  <block type="system">', lines[1]
            assert_equal '    System prompt', lines[2]
            assert_equal '  </block>', lines[3]
            assert_equal '  <block type="user">', lines[4]
            assert_equal '    User message', lines[5]
            assert_equal '  </block>', lines[6]
            assert_equal '</context>', lines[7]
          end

          def test_render_builder_with_filesystem
            builder = Context.build do
              filesystem do
                file 'config.yml', content: 'key: value'
                directory 'src' do
                  file 'main.rb', content: 'puts "hello"'
                end
              end
              block :system, 'System prompt'
            end

            output = @renderer.render(builder)
            lines = output.split("\n")

            assert_equal '<context>', lines[0]
            # Filesystem starts
            assert_equal '  <filesystem name="/">', lines[1]
            # config.yml
            assert_equal '    <file name="config.yml">', lines[2]
            assert_equal '      key: value', lines[3]
            assert_equal '    </file>', lines[4]
            # src directory
            assert_equal '    <filesystem name="src">', lines[5]
            assert_equal '      <file name="main.rb">', lines[6]
            assert_equal '        puts &quot;hello&quot;', lines[7]
            assert_equal '      </file>', lines[8]
            assert_equal '    </filesystem>', lines[9]
            # Close root filesystem
            assert_equal '  </filesystem>', lines[10]
            # Block
            assert_equal '  <block type="system">', lines[11]
            assert_equal '    System prompt', lines[12]
            assert_equal '  </block>', lines[13]
            assert_equal '</context>', lines[14]
          end

          def test_render_empty_builder
            builder = Context.build
            output = @renderer.render(builder)
            expected = "<context>\n</context>"
            assert_equal expected, output
          end

          def test_render_builder_only_filesystem
            builder = Context.build do
              filesystem do
                file 'test.txt', content: 'content'
              end
            end

            output = @renderer.render(builder)
            assert_includes output, '<context>'
            assert_includes output, '<filesystem name="/">'
            assert_includes output, '<file name="test.txt">'
            assert_includes output, 'content'
            assert_includes output, '</context>'
          end

          def test_render_builder_only_blocks
            builder = Context.build do
              block :test, 'content'
            end

            output = @renderer.render(builder)
            assert_includes output, '<context>'
            assert_includes output, '<block type="test">'
            assert_includes output, 'content'
            assert_includes output, '</context>'
          end

          def test_render_large_content
            large_content = 'a' * 10_000
            builder = Context.build do
              block :large, large_content
            end

            output = @renderer.render(builder)
            assert_includes output, large_content
            assert_includes output, '<block type="large">'
          end

          def test_render_special_characters_everywhere
            builder = Context.build do
              filesystem do
                file 'file & "name".txt', content: 'Content & < > " \''
                directory 'dir & <name>' do
                  file 'nested & file.txt', content: 'Nested & content'
                end
              end
              block :'type & name', 'Block & <content> "quotes" \'single\''
            end

            output = @renderer.render(builder)

            # Check escaping in filesystem name
            assert_includes output, 'name="dir &amp; &lt;name&gt;"'
            # Check escaping in file names
            assert_includes output, 'name="file &amp; &quot;name&quot;.txt"'
            assert_includes output, 'name="nested &amp; file.txt"'
            # Check escaping in file content
            assert_includes output, 'Content &amp; &lt; &gt; &quot; &apos;'
            assert_includes output, 'Nested &amp; content'
            # Check escaping in block type
            assert_includes output, 'type="type &amp; name"'
            # Check escaping in block content
            assert_includes output, 'Block &amp; &lt;content&gt; &quot;quotes&quot; &apos;single&apos;'
          end

          def test_render_nil_content
            builder = Context.build do
              block :nil_test, nil
            end

            output = @renderer.render(builder)
            assert_includes output, '<block type="nil_test">'
            assert_includes output, '</block>'
            # nil.to_s is empty string, so content line is just indentation
            lines = output.split("\n")
            block_start = lines.index('  <block type="nil_test">')
            assert_equal '    ', lines[block_start + 1] # empty content line
            assert_equal '  </block>', lines[block_start + 2] # closing tag
          end

          def test_render_empty_filesystem_name
            fs = Filesystem.new('')
            output = @renderer.render_filesystem(fs)
            assert_includes output, '<filesystem name="">'
          end

          def test_render_filesystem_with_nil_name
            fs = Filesystem.new(nil)
            output = @renderer.render_filesystem(fs)
            assert_includes output, '<filesystem name="">'
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
