# frozen_string_literal: true

require 'minitest/autorun'
require 'mocha/minitest'

# Add lib to load path
$LOAD_PATH.unshift File.expand_path('../../lib', __dir__)

# Mock the cosmos-llm-tool dependency before requiring anything
module Cosmos
  module Llm
    module Tool
      def self.define(name, register: false, &block)
        # Create a mock tool definition that can execute the block
        tool = MockToolDefinition.new(name)
        tool.instance_eval(&block) if block
        tool
      end

      # Mock the Definition class
      class Definition
        def self.new(*args)
          MockToolDefinition.new(*args)
        end
      end

      class MockToolDefinition
        def initialize(name, &block)
          @name = name
          @block = block
          @parameters = []
        end

        def description(desc)
          @description = desc
        end

        def parameter(name, **options)
          @parameters << { name: name, **options }
        end

        def execute(&block)
          @execute_block = block
        end

        def call(params = {})
          # Execute the stored block with the parameters
          @execute_block.call(params) if @execute_block
        end

        def to_openai_schema
          {
            'function' => {
              'name' => @name.to_s,
              'description' => @description,
              'parameters' => {
                'type' => 'object',
                'properties' => @parameters.each_with_object({}) do |param, hash|
                   hash[param[:name].to_s] = {
                     'type' => param[:type].to_s,
                     'description' => param[:description]
                   }
                   hash[param[:name].to_s]['required'] = param[:required] if param.key?(:required)
                 end,
                 'required' => @parameters.select { |p| p[:required] }.map { |p| p[:name].to_s }
              }
            }
          }
        end

        def to_anthropic_schema
          # Mock anthropic schema
          { 'name' => @name.to_s, 'description' => @description }
        end
      end
    end
  end
end

# Mock the require to avoid loading the actual gem
$LOADED_FEATURES << 'cosmos/llm/tool.rb'

require_relative '../../lib/cosmos/llm/tool/preset'

module Cosmos
  module Llm
    module Tool
      class PresetTest < Minitest::Test
        def setup
          @mock_filesystem = mock('filesystem')
        end

        def test_module_exists
          assert defined?(Cosmos::Llm::Tool::Preset)
        end

        def test_read_method_exists
          assert_respond_to Cosmos::Llm::Tool::Preset, :read
        end

        def test_read_returns_tool_definition
          tool = Cosmos::Llm::Tool::Preset.read(@mock_filesystem)
          assert_kind_of Cosmos::Llm::Tool::MockToolDefinition, tool
        end

        def test_write_method_exists
          assert_respond_to Cosmos::Llm::Tool::Preset, :write
        end

        def test_write_returns_tool_definition
          tool = Cosmos::Llm::Tool::Preset.write(@mock_filesystem)
          assert_kind_of Cosmos::Llm::Tool::MockToolDefinition, tool
        end

        def test_list_method_exists
          assert_respond_to Cosmos::Llm::Tool::Preset, :list
        end

        def test_list_returns_tool_definition
          tool = Cosmos::Llm::Tool::Preset.list(@mock_filesystem)
          assert_kind_of Cosmos::Llm::Tool::MockToolDefinition, tool
        end

        def test_glob_method_exists
          assert_respond_to Cosmos::Llm::Tool::Preset, :glob
        end

        def test_glob_returns_tool_definition
          tool = Cosmos::Llm::Tool::Preset.glob(@mock_filesystem)
          assert_kind_of Cosmos::Llm::Tool::MockToolDefinition, tool
        end

        def test_grep_method_exists
          assert_respond_to Cosmos::Llm::Tool::Preset, :grep
        end

        def test_grep_returns_tool_definition
          tool = Cosmos::Llm::Tool::Preset.grep(@mock_filesystem)
          assert_kind_of Cosmos::Llm::Tool::MockToolDefinition, tool
        end

        def test_jq_method_exists
          assert_respond_to Cosmos::Llm::Tool::Preset, :jq
        end

        def test_jq_returns_tool_definition
          tool = Cosmos::Llm::Tool::Preset.jq(@mock_filesystem)
          assert_kind_of Cosmos::Llm::Tool::MockToolDefinition, tool
        end

        def test_webfetch_method_exists
          assert_respond_to Cosmos::Llm::Tool::Preset, :webfetch
        end

        def test_webfetch_returns_tool_definition
          tool = Cosmos::Llm::Tool::Preset.webfetch
          assert_kind_of Cosmos::Llm::Tool::MockToolDefinition, tool
        end
       end
     end

     class GrepTest < Minitest::Test
        def setup
          @mock_filesystem = mock('filesystem')
          @grep_tool = Cosmos::Llm::Tool::Preset.grep(@mock_filesystem)
        end

        def test_grep_tool_creation
          assert_kind_of Cosmos::Llm::Tool::MockToolDefinition, @grep_tool
        end

        def test_grep_tool_schema
          schema = @grep_tool.to_openai_schema
          assert_equal 'grep', schema['function']['name']
          assert_includes schema['function']['description'], 'regular expressions'
          assert_includes schema['function']['parameters']['properties'], 'pattern'
          assert_includes schema['function']['parameters']['properties'], 'file_pattern'
        end

        def test_grep_simple_pattern
          mock_file1 = mock('file1')
          mock_file1.stubs(:content).returns("line 1\nline with test\nline 3")
          mock_file2 = mock('file2')
          mock_file2.stubs(:content).returns("other content\nmore test here")

          @mock_filesystem.stubs(:all_files).returns([
                                                       { file: mock_file1, path: 'file1.txt' },
                                                       { file: mock_file2, path: 'file2.txt' }
                                                     ])

          result = @grep_tool.call(pattern: 'test')

          assert result[:success]
          assert_equal 'test', result[:pattern]
          assert_equal 2, result[:match_count]
          assert_equal 2, result[:files_with_matches]
          assert_equal 2, result[:files_searched]
          assert_equal 2, result[:matches].length

          # Check first match
          match1 = result[:matches][0]
          assert_equal 'file1.txt', match1[:file]
          assert_equal 2, match1[:line_number]
          assert_equal 'line with test', match1[:content].strip
          assert_equal 'test', match1[:match]
        end

        def test_grep_no_matches
          mock_file = mock('file')
          mock_file.stubs(:content).returns("line 1\nline 2\nline 3")

          @mock_filesystem.stubs(:all_files).returns([
                                                       { file: mock_file, path: 'test.txt' }
                                                     ])

          result = @grep_tool.call(pattern: 'nonexistent')

          assert result[:success]
          assert_equal 'nonexistent', result[:pattern]
          assert_equal 0, result[:match_count]
          assert_equal 0, result[:files_with_matches]
          assert_equal 1, result[:files_searched]
          assert_empty result[:matches]
        end

        def test_grep_with_file_pattern
          mock_file1 = mock('file1')
          mock_file1.stubs(:content).returns('ruby code here')
          mock_file2 = mock('file2')
          mock_file2.stubs(:content).returns('ruby code there')

          @mock_filesystem.stubs(:all_files).returns([
                                                       { file: mock_file1, path: 'src/main.rb' },
                                                       { file: mock_file2, path: 'src/utils.js' }
                                                     ])

          result = @grep_tool.call(pattern: 'ruby', file_pattern: '*.rb')

          assert result[:success]
          assert_equal 1, result[:match_count]
          assert_equal 1, result[:files_with_matches]
          assert_equal 1, result[:files_searched] # Only .rb files searched
          assert_equal 1, result[:matches].length
          assert_equal 'src/main.rb', result[:matches][0][:file]
        end

        def test_grep_complex_glob_patterns
          mock_file1 = mock('file1')
          mock_file1.stubs(:content).returns('content1')
          mock_file2 = mock('file2')
          mock_file2.stubs(:content).returns('content2')
          mock_file3 = mock('file3')
          mock_file3.stubs(:content).returns('content3')

          @mock_filesystem.stubs(:all_files).returns([
                                                       { file: mock_file1, path: 'src/main.rb' },
                                                       { file: mock_file2, path: 'src/lib/utils.rb' },
                                                       { file: mock_file3, path: 'test/main_test.rb' }
                                                     ])

          # Test ** glob pattern
          result = @grep_tool.call(pattern: 'content', file_pattern: 'src/**/*.rb')
          assert_equal 1, result[:files_searched] # src/lib/utils.rb (main.rb is not in a subdirectory)

          # Test alternation
          result = @grep_tool.call(pattern: 'content', file_pattern: '**/*.{rb,js}')
          assert_equal 3, result[:files_searched] # All files match
        end

        def test_grep_regex_patterns
          mock_file = mock('file')
          mock_file.stubs(:content).returns("test123\ntest456\nno match")

          @mock_filesystem.stubs(:all_files).returns([
                                                       { file: mock_file, path: 'test.txt' }
                                                     ])

          result = @grep_tool.call(pattern: 'test\d+')

          assert result[:success]
          assert_equal 2, result[:match_count]
          assert_equal 'test123', result[:matches][0][:match]
          assert_equal 'test456', result[:matches][1][:match]
        end

        def test_grep_case_insensitive
          mock_file = mock('file')
          mock_file.stubs(:content).returns("Test\nTEST\ntest")

          @mock_filesystem.stubs(:all_files).returns([
                                                       { file: mock_file, path: 'test.txt' }
                                                     ])

          result = @grep_tool.call(pattern: '(?i)test')

          assert result[:success]
          assert_equal 3, result[:match_count]
        end

        def test_grep_empty_file
          mock_file = mock('file')
          mock_file.stubs(:content).returns('')

          @mock_filesystem.stubs(:all_files).returns([
                                                       { file: mock_file, path: 'empty.txt' }
                                                     ])

          result = @grep_tool.call(pattern: 'test')

          assert result[:success]
          assert_equal 0, result[:match_count]
          assert_equal 1, result[:files_searched]
          assert_empty result[:matches]
        end

        def test_grep_nil_content
          mock_file = mock('file')
          mock_file.stubs(:content).returns(nil)

          @mock_filesystem.stubs(:all_files).returns([
                                                       { file: mock_file, path: 'nil.txt' }
                                                     ])

          result = @grep_tool.call(pattern: 'test')

          assert result[:success]
          assert_equal 0, result[:match_count]
          assert_equal 0, result[:files_searched] # File skipped due to nil content
          assert_empty result[:matches]
        end

        def test_grep_binary_file
          mock_file = mock('file')
          mock_file.stubs(:content).returns("text content\x00binary data")

          @mock_filesystem.stubs(:all_files).returns([
                                                       { file: mock_file, path: 'binary.dat' }
                                                     ])

          result = @grep_tool.call(pattern: 'text')

          assert result[:success]
          assert_equal 0, result[:match_count]
          assert_equal 0, result[:files_searched] # Binary file skipped
          assert_empty result[:matches]
        end

        def test_grep_invalid_regex
          @mock_filesystem.stubs(:all_files).returns([])

          result = @grep_tool.call(pattern: '[invalid')

          refute result[:success]
          assert_includes result[:error], 'Invalid regex pattern'
          assert_equal '[invalid', result[:pattern]
        end

        def test_grep_exception_handling
          @mock_filesystem.stubs(:all_files).raises(StandardError.new('filesystem error'))

          result = @grep_tool.call(pattern: 'test')

          refute result[:success]
          assert_equal 'filesystem error', result[:error]
          assert_equal 'test', result[:pattern]
        end

        def test_grep_multiple_matches_per_line
          mock_file = mock('file')
          mock_file.stubs(:content).returns("test test\ntest")

          @mock_filesystem.stubs(:all_files).returns([
                                                       { file: mock_file, path: 'test.txt' }
                                                     ])

          result = @grep_tool.call(pattern: 'test')

          assert result[:success]
          assert_equal 3, result[:match_count] # 2 on first line, 1 on second
          assert_equal 1, result[:files_with_matches] # Only 1 file has matches
        end

        def test_grep_capture_groups
          mock_file = mock('file')
          mock_file.stubs(:content).returns("name: alice\nname: bob")

          @mock_filesystem.stubs(:all_files).returns([
                                                       { file: mock_file, path: 'test.txt' }
                                                     ])

          result = @grep_tool.call(pattern: 'name: (\w+)')

          assert result[:success]
          assert_equal 2, result[:match_count]
          assert_equal 'name: alice', result[:matches][0][:match]
          assert_equal 'name: bob', result[:matches][1][:match]
        end

        def test_grep_line_numbers
          mock_file = mock('file')
          mock_file.stubs(:content).returns("line1\nline2\nmatch\nline4")

          @mock_filesystem.stubs(:all_files).returns([
                                                       { file: mock_file, path: 'test.txt' }
                                                     ])

          result = @grep_tool.call(pattern: 'match')

          assert result[:success]
          assert_equal 1, result[:match_count]
          assert_equal 3, result[:matches][0][:line_number]
          assert_equal 'match', result[:matches][0][:content].strip
        end

        def test_grep_large_file
          large_content = ("line\n" * 1000) + "match\n" + ("line\n" * 1000)
          mock_file = mock('file')
          mock_file.stubs(:content).returns(large_content)

          @mock_filesystem.stubs(:all_files).returns([
                                                       { file: mock_file, path: 'large.txt' }
                                                     ])

          result = @grep_tool.call(pattern: 'match')

          assert result[:success]
          assert_equal 1, result[:match_count]
          assert_equal 1001, result[:matches][0][:line_number]
        end

        def test_grep_whitespace_and_newlines
          mock_file = mock('file')
          mock_file.stubs(:content).returns("  match  \n\tmatch\t\nmatch")

          @mock_filesystem.stubs(:all_files).returns([
                                                       { file: mock_file, path: 'test.txt' }
                                                     ])

          result = @grep_tool.call(pattern: 'match')

          assert result[:success]
          assert_equal 3, result[:match_count]
          assert_equal '  match  ', result[:matches][0][:content]
          assert_equal "\tmatch\t", result[:matches][1][:content]
          assert_equal 'match', result[:matches][2][:content]
        end
       end
     end

     class ReadTest < Minitest::Test
       def setup
         @mock_filesystem = mock('filesystem')
         @read_tool = Cosmos::Llm::Tool::Preset.read(@mock_filesystem)
       end

       def test_read_tool_creation
         assert_kind_of Cosmos::Llm::Tool::MockToolDefinition, @read_tool
       end

       def test_read_tool_schema
            schema = @read_tool.to_openai_schema
            assert_equal 'read', schema['function']['name']
            assert_includes schema['function']['description'], 'Read file contents'
            assert_includes schema['function']['parameters']['properties'], 'file_path'
            assert_includes schema['function']['parameters']['properties'], 'offset'
            assert_includes schema['function']['parameters']['properties'], 'limit'
          end

          def test_read_text_file
            mock_file = mock('file')
            content = "line 1\nline 2\nline 3"
            mock_file.stubs(:content).returns(content)
            mock_file.stubs(:attributes).returns({ size: content.bytesize })

            @mock_filesystem.stubs(:find_file).with('test.txt').returns(mock_file)

            result = @read_tool.call(file_path: 'test.txt')

            assert result[:success]
            assert_equal 'test.txt', result[:file_path]
            assert_includes result[:content], '1	line 1'
            assert_includes result[:content], '2	line 2'
            assert_includes result[:content], '3	line 3'
            assert_equal 3, result[:total_lines]
            assert_equal 3, result[:read_lines]
            assert_equal 1, result[:start_line]
            assert_equal 3, result[:end_line]
            assert_equal content.bytesize, result[:size]
          end

          def test_read_file_with_offset
            mock_file = mock('file')
            mock_file.stubs(:content).returns("line 1\nline 2\nline 3\nline 4")
            mock_file.stubs(:attributes).returns({})

            @mock_filesystem.stubs(:find_file).with('test.txt').returns(mock_file)

            result = @read_tool.call(file_path: 'test.txt', offset: 1)

            assert result[:success]
            assert_equal 4, result[:total_lines]
            assert_equal 3, result[:read_lines]
            assert_equal 2, result[:start_line] # 1-based
            assert_equal 4, result[:end_line]
            assert_includes result[:content], '2	line 2'
            refute_includes result[:content], '1	line 1'
          end

          def test_read_file_with_limit
            mock_file = mock('file')
            mock_file.stubs(:content).returns("line 1\nline 2\nline 3\nline 4")
            mock_file.stubs(:attributes).returns({})

            @mock_filesystem.stubs(:find_file).with('test.txt').returns(mock_file)

            result = @read_tool.call(file_path: 'test.txt', limit: 2)

            assert result[:success]
            assert_equal 4, result[:total_lines]
            assert_equal 2, result[:read_lines]
            assert_equal 1, result[:start_line]
            assert_equal 2, result[:end_line]
            assert_includes result[:content], '1	line 1'
            assert_includes result[:content], '2	line 2'
            refute_includes result[:content], '3	line 3'
          end

          def test_read_file_with_offset_and_limit
            mock_file = mock('file')
            mock_file.stubs(:content).returns("line 1\nline 2\nline 3\nline 4\nline 5")
            mock_file.stubs(:attributes).returns({})

            @mock_filesystem.stubs(:find_file).with('test.txt').returns(mock_file)

            result = @read_tool.call(file_path: 'test.txt', offset: 1, limit: 2)

            assert result[:success]
            assert_equal 5, result[:total_lines]
            assert_equal 2, result[:read_lines]
            assert_equal 2, result[:start_line]
            assert_equal 3, result[:end_line]
            assert_includes result[:content], '2	line 2'
            assert_includes result[:content], '3	line 3'
          end

          def test_read_binary_file
            mock_file = mock('file')
            binary_content = "text\x00binary"
            mock_file.stubs(:content).returns(binary_content)
            mock_file.stubs(:attributes).returns({})

            @mock_filesystem.stubs(:find_file).with('binary.dat').returns(mock_file)

            result = @read_tool.call(file_path: 'binary.dat')

            assert result[:success]
            assert_equal binary_content, result[:content]
            assert_equal 1, result[:total_lines]
            assert_equal 1, result[:read_lines]
            assert_equal 1, result[:start_line]
            assert_equal 1, result[:end_line]
          end

          def test_read_file_not_found
            @mock_filesystem.stubs(:find_file).with('missing.txt').returns(nil)

            result = @read_tool.call(file_path: 'missing.txt')

            refute result[:success]
            assert_equal 'File not found in virtual filesystem', result[:error]
            assert_equal 'missing.txt', result[:file_path]
          end

          def test_read_empty_file
            mock_file = mock('file')
            mock_file.stubs(:content).returns('')
            mock_file.stubs(:attributes).returns({})

            @mock_filesystem.stubs(:find_file).with('empty.txt').returns(mock_file)

            result = @read_tool.call(file_path: 'empty.txt')

            assert result[:success]
            assert_equal '', result[:content]
            assert_equal 0, result[:total_lines] # Empty string has 0 lines
            assert_equal 0, result[:read_lines]
            assert_equal 1, result[:start_line]
            assert_equal 0, result[:end_line]
          end

          def test_read_file_with_negative_offset
            mock_file = mock('file')
            mock_file.stubs(:content).returns("line 1\nline 2")
            mock_file.stubs(:attributes).returns({})

            @mock_filesystem.stubs(:find_file).with('test.txt').returns(mock_file)

            result = @read_tool.call(file_path: 'test.txt', offset: -1)

            assert result[:success]
            assert_equal 1, result[:start_line] # Negative offset treated as 0
            assert_equal 2, result[:read_lines]
          end

           def test_read_file_exception_handling
             mock_file = mock('file')
             mock_file.stubs(:content).raises(StandardError.new('content error'))
             @mock_filesystem.stubs(:find_file).returns(mock_file)

             result = @read_tool.call(file_path: 'test.txt')

             refute result[:success]
             assert_equal 'content error', result[:error]
           end








        end

      class WriteTest < Minitest::Test
          def setup
            @mock_filesystem = mock('filesystem')
            @write_tool = Cosmos::Llm::Tool::Preset.write(@mock_filesystem)
          end

          def test_write_tool_creation
            assert_kind_of Cosmos::Llm::Tool::MockToolDefinition, @write_tool
          end

          def test_write_tool_schema
            schema = @write_tool.to_openai_schema
            assert_equal 'write', schema['function']['name']
            assert_includes schema['function']['description'], 'Write content to a file'
            assert_includes schema['function']['parameters']['properties'], 'file_path'
            assert_includes schema['function']['parameters']['properties'], 'content'
          end

          def test_write_new_file
            content = 'new file content'
            @mock_filesystem.stubs(:find_file).with('new.txt').returns(nil)

            result = @write_tool.call(file_path: 'new.txt', content: content)

            assert result[:success]
            assert_equal 'new.txt', result[:file_path]
            assert_equal content, result[:content]
            assert_equal content.bytesize, result[:size]
            assert result[:created]
            refute result[:updated]
            assert_nil result[:previous_size]
          end

          def test_write_existing_file
            content = 'updated content'
            mock_existing_file = mock('existing_file')
            mock_existing_file.stubs(:content).returns('old content')
            @mock_filesystem.stubs(:find_file).with('existing.txt').returns(mock_existing_file)

            result = @write_tool.call(file_path: 'existing.txt', content: content)

            assert result[:success]
            assert_equal 'existing.txt', result[:file_path]
            assert_equal content, result[:content]
            assert_equal content.bytesize, result[:size]
            refute result[:created]
            assert result[:updated]
            assert_equal 11, result[:previous_size] # "old content".bytesize
          end

          def test_write_empty_content
            @mock_filesystem.stubs(:find_file).with('empty.txt').returns(nil)

            result = @write_tool.call(file_path: 'empty.txt', content: '')

            assert result[:success]
            assert_equal 'empty.txt', result[:file_path]
            assert_equal '', result[:content]
            assert_equal 0, result[:size]
            assert result[:created]
            refute result[:updated]
            assert_nil result[:previous_size]
          end

          def test_write_binary_content
            binary_content = "text\x00binary\x01data"
            @mock_filesystem.stubs(:find_file).with('binary.dat').returns(nil)

            result = @write_tool.call(file_path: 'binary.dat', content: binary_content)

            assert result[:success]
            assert_equal 'binary.dat', result[:file_path]
            assert_equal binary_content, result[:content]
            assert_equal binary_content.bytesize, result[:size]
          end

           def test_write_exception_handling
             @mock_filesystem.stubs(:find_file).raises(StandardError.new('filesystem error'))

             result = @write_tool.call(file_path: 'test.txt', content: 'content')

             refute result[:success]
             assert_equal 'filesystem error', result[:error]
             assert_equal 'test.txt', result[:file_path]
           end

           def test_write_with_nil_file_path
             result = @write_tool.call(file_path: nil, content: 'content')

             refute result[:success]
             assert_includes result[:error], 'file_path parameter is required'
           end

           def test_write_with_empty_file_path
             result = @write_tool.call(file_path: '', content: 'content')

             refute result[:success]
             assert_includes result[:error], 'file_path parameter is required'
           end

           def test_write_with_non_string_file_path
             result = @write_tool.call(file_path: 123, content: 'content')

             refute result[:success]
             assert_includes result[:error], 'file_path parameter is required'
           end

           def test_write_with_nil_content
             result = @write_tool.call(file_path: 'test.txt', content: nil)

             refute result[:success]
             assert_includes result[:error], 'content parameter must be a string'
           end

           def test_write_with_non_string_content
             result = @write_tool.call(file_path: 'test.txt', content: 12345)

             refute result[:success]
             assert_includes result[:error], 'content parameter must be a string'
           end

           def test_write_with_unicode_content
             unicode_content = 'Hello 世界 🌍'
             @mock_filesystem.stubs(:find_file).with('unicode.txt').returns(nil)

             result = @write_tool.call(file_path: 'unicode.txt', content: unicode_content)

             assert result[:success]
             assert_equal unicode_content, result[:content]
             assert_equal unicode_content.bytesize, result[:size]
           end

           def test_write_large_content
             large_content = 'a' * 1000000 # 1MB of content
             @mock_filesystem.stubs(:find_file).with('large.txt').returns(nil)

             result = @write_tool.call(file_path: 'large.txt', content: large_content)

             assert result[:success]
             assert_equal large_content, result[:content]
             assert_equal 1000000, result[:size]
           end

           def test_write_with_special_characters_in_path
             special_path = 'dir/file-name_with.special.chars.txt'
             @mock_filesystem.stubs(:find_file).with(special_path).returns(nil)

             result = @write_tool.call(file_path: special_path, content: 'content')

             assert result[:success]
             assert_equal special_path, result[:file_path]
           end

           def test_write_existing_file_with_nil_content
             mock_existing_file = mock('existing_file')
             mock_existing_file.stubs(:content).returns(nil)
             @mock_filesystem.stubs(:find_file).with('nil_content.txt').returns(mock_existing_file)

             result = @write_tool.call(file_path: 'nil_content.txt', content: 'new content')

             assert result[:success]
             assert result[:updated]
             refute result[:created]
             assert_nil result[:previous_size] # nil.content is nil, so &. returns nil
           end

           def test_write_with_whitespace_only_content
             content = "   \n\t  "
             @mock_filesystem.stubs(:find_file).with('whitespace.txt').returns(nil)

             result = @write_tool.call(file_path: 'whitespace.txt', content: content)

             assert result[:success]
             assert_equal content, result[:content]
             assert_equal content.bytesize, result[:size]
           end

           def test_write_argument_error_handling
             # Test that ArgumentError is caught and returned as error
             @mock_filesystem.stubs(:find_file).raises(ArgumentError.new('validation error'))

             result = @write_tool.call(file_path: 'test.txt', content: 'content')

             refute result[:success]
             assert_equal 'validation error', result[:error]
           end
        end

      class ListTest < Minitest::Test
          def setup
            @mock_filesystem = mock('filesystem')
            @list_tool = Cosmos::Llm::Tool::Preset.list(@mock_filesystem)
          end

          def test_list_tool_creation
            assert_kind_of Cosmos::Llm::Tool::MockToolDefinition, @list_tool
          end

          def test_list_tool_schema
            schema = @list_tool.to_openai_schema
            assert_equal 'list', schema['function']['name']
            assert_includes schema['function']['description'], 'List all files'
            assert_includes schema['function']['parameters']['properties'], 'pattern'
          end

          def test_list_all_files
            mock_file1 = mock('file1')
            mock_file1.stubs(:name).returns('main.rb')
            mock_file1.stubs(:content).returns('puts "hello"')
            mock_file1.stubs(:attributes).returns({ type: 'file' })

            mock_file2 = mock('file2')
            mock_file2.stubs(:name).returns('helper.rb')
            mock_file2.stubs(:content).returns('def help; end')
            mock_file2.stubs(:attributes).returns({ type: 'file' })

            @mock_filesystem.stubs(:all_files).returns([
                                                         { file: mock_file1, path: 'src/main.rb' },
                                                         { file: mock_file2, path: 'src/helper.rb' }
                                                       ])

            result = @list_tool.call

            assert result[:success]
            assert_equal 2, result[:count]
            assert_nil result[:pattern]
            assert_equal 2, result[:files].length

            file1 = result[:files][0]
            assert_equal 'src/main.rb', file1[:path]
            assert_equal 'main.rb', file1[:name]
            assert_equal 'puts "hello"'.bytesize, file1[:size]
            assert_equal({ type: 'file' }, file1[:attributes])

            file2 = result[:files][1]
            assert_equal 'src/helper.rb', file2[:path]
            assert_equal 'helper.rb', file2[:name]
            assert_equal 'def help; end'.bytesize, file2[:size]
          end

          def test_list_files_with_pattern
            mock_file1 = mock('file1')
            mock_file1.stubs(:name).returns('main.rb')
            mock_file1.stubs(:content).returns('code')
            mock_file1.stubs(:attributes).returns({})

            mock_file2 = mock('file2')
            mock_file2.stubs(:name).returns('main.js')
            mock_file2.stubs(:content).returns('code')
            mock_file2.stubs(:attributes).returns({})

            @mock_filesystem.stubs(:all_files).returns([
                                                         { file: mock_file1, path: 'src/main.rb' },
                                                         { file: mock_file2, path: 'src/main.js' }
                                                       ])

            result = @list_tool.call(pattern: '*.rb')

            assert result[:success]
            assert_equal 1, result[:count]
            assert_equal '*.rb', result[:pattern]
            assert_equal 1, result[:files].length
            assert_equal 'src/main.rb', result[:files][0][:path]
          end

          def test_list_files_empty_filesystem
            @mock_filesystem.stubs(:all_files).returns([])

            result = @list_tool.call

            assert result[:success]
            assert_equal 0, result[:count]
            assert_empty result[:files]
          end

          def test_list_files_with_nil_content
            mock_file = mock('file')
            mock_file.stubs(:name).returns('empty.txt')
            mock_file.stubs(:content).returns(nil)
            mock_file.stubs(:attributes).returns({})

            @mock_filesystem.stubs(:all_files).returns([
                                                         { file: mock_file, path: 'empty.txt' }
                                                       ])

            result = @list_tool.call

            assert result[:success]
            assert_equal 1, result[:count]
            assert_equal 0, result[:files][0][:size] # nil content should be 0 size
          end

           def test_list_files_exception_handling
             @mock_filesystem.stubs(:all_files).raises(StandardError.new('filesystem error'))

             result = @list_tool.call

             refute result[:success]
             assert_equal 'filesystem error', result[:error]
           end

           def test_list_files_with_recursive_glob_pattern
             mock_file1 = mock('file1')
             mock_file1.stubs(:name).returns('main.rb')
             mock_file1.stubs(:content).returns('code')
             mock_file1.stubs(:attributes).returns({})

             mock_file2 = mock('file2')
             mock_file2.stubs(:name).returns('helper.rb')
             mock_file2.stubs(:content).returns('code')
             mock_file2.stubs(:attributes).returns({})

             mock_file3 = mock('file3')
             mock_file3.stubs(:name).returns('deep.rb')
             mock_file3.stubs(:content).returns('code')
             mock_file3.stubs(:attributes).returns({})

             @mock_filesystem.stubs(:all_files).returns([
                                                          { file: mock_file1, path: 'src/main.rb' },
                                                          { file: mock_file2, path: 'src/lib/helper.rb' },
                                                          { file: mock_file3, path: 'src/lib/sub/deep.rb' }
                                                        ])

             result = @list_tool.call(pattern: 'src/**/*.rb')

             assert result[:success]
             assert_equal 3, result[:count]
             assert_equal 'src/**/*.rb', result[:pattern]
             paths = result[:files].map { |f| f[:path] }
             assert_includes paths, 'src/main.rb'
             assert_includes paths, 'src/lib/helper.rb'
             assert_includes paths, 'src/lib/sub/deep.rb'
           end

           def test_list_files_with_alternation_pattern
             mock_file1 = mock('file1')
             mock_file1.stubs(:name).returns('main.rb')
             mock_file1.stubs(:content).returns('code')
             mock_file1.stubs(:attributes).returns({})

             mock_file2 = mock('file2')
             mock_file2.stubs(:name).returns('main.js')
             mock_file2.stubs(:content).returns('code')
             mock_file2.stubs(:attributes).returns({})

             mock_file3 = mock('file3')
             mock_file3.stubs(:name).returns('main.py')
             mock_file3.stubs(:content).returns('code')
             mock_file3.stubs(:attributes).returns({})

             @mock_filesystem.stubs(:all_files).returns([
                                                          { file: mock_file1, path: 'main.rb' },
                                                          { file: mock_file2, path: 'main.js' },
                                                          { file: mock_file3, path: 'main.py' }
                                                        ])

             result = @list_tool.call(pattern: 'main.{rb,js}')

             assert result[:success]
             assert_equal 2, result[:count]
             paths = result[:files].map { |f| f[:path] }
             assert_includes paths, 'main.rb'
             assert_includes paths, 'main.js'
             refute_includes paths, 'main.py'
           end

           def test_list_files_with_no_matches
             mock_file = mock('file')
             mock_file.stubs(:name).returns('main.js')
             mock_file.stubs(:content).returns('code')
             mock_file.stubs(:attributes).returns({})

             @mock_filesystem.stubs(:all_files).returns([
                                                          { file: mock_file, path: 'main.js' }
                                                        ])

             result = @list_tool.call(pattern: '*.rb')

             assert result[:success]
             assert_equal 0, result[:count]
             assert_empty result[:files]
           end

           def test_list_files_with_empty_pattern
             mock_file = mock('file')
             mock_file.stubs(:name).returns('main.rb')
             mock_file.stubs(:content).returns('code')
             mock_file.stubs(:attributes).returns({})

             @mock_filesystem.stubs(:all_files).returns([
                                                          { file: mock_file, path: 'main.rb' }
                                                        ])

             result = @list_tool.call(pattern: '')

             assert result[:success]
             assert_equal 0, result[:count] # Empty pattern should match nothing
             assert_empty result[:files]
           end

           def test_list_files_with_question_mark_pattern
             mock_file1 = mock('file1')
             mock_file1.stubs(:name).returns('file1.txt')
             mock_file1.stubs(:content).returns('content')
             mock_file1.stubs(:attributes).returns({})

             mock_file2 = mock('file2')
             mock_file2.stubs(:name).returns('file2.txt')
             mock_file2.stubs(:content).returns('content')
             mock_file2.stubs(:attributes).returns({})

             mock_file3 = mock('file3')
             mock_file3.stubs(:name).returns('file10.txt')
             mock_file3.stubs(:content).returns('content')
             mock_file3.stubs(:attributes).returns({})

             @mock_filesystem.stubs(:all_files).returns([
                                                          { file: mock_file1, path: 'file1.txt' },
                                                          { file: mock_file2, path: 'file2.txt' },
                                                          { file: mock_file3, path: 'file10.txt' }
                                                        ])

             result = @list_tool.call(pattern: 'file?.txt')

             assert result[:success]
             assert_equal 2, result[:count]
             paths = result[:files].map { |f| f[:path] }
             assert_includes paths, 'file1.txt'
             assert_includes paths, 'file2.txt'
             refute_includes paths, 'file10.txt'
           end

           def test_list_files_with_character_class_pattern
             mock_file1 = mock('file1')
             mock_file1.stubs(:name).returns('file1.txt')
             mock_file1.stubs(:content).returns('content')
             mock_file1.stubs(:attributes).returns({})

             mock_file2 = mock('file2')
             mock_file2.stubs(:name).returns('file2.txt')
             mock_file2.stubs(:content).returns('content')
             mock_file2.stubs(:attributes).returns({})

             mock_file3 = mock('file3')
             mock_file3.stubs(:name).returns('file3.txt')
             mock_file3.stubs(:content).returns('content')
             mock_file3.stubs(:attributes).returns({})

             @mock_filesystem.stubs(:all_files).returns([
                                                          { file: mock_file1, path: 'file1.txt' },
                                                          { file: mock_file2, path: 'file2.txt' },
                                                          { file: mock_file3, path: 'file3.txt' }
                                                        ])

             result = @list_tool.call(pattern: 'file[12].txt')

             assert result[:success]
             assert_equal 2, result[:count]
             paths = result[:files].map { |f| f[:path] }
             assert_includes paths, 'file1.txt'
             assert_includes paths, 'file2.txt'
             refute_includes paths, 'file3.txt'
           end

           def test_list_files_with_literal_dot_pattern
             mock_file1 = mock('file1')
             mock_file1.stubs(:name).returns('main.rb')
             mock_file1.stubs(:content).returns('content')
             mock_file1.stubs(:attributes).returns({})

             mock_file2 = mock('file2')
             mock_file2.stubs(:name).returns('main.js')
             mock_file2.stubs(:content).returns('content')
             mock_file2.stubs(:attributes).returns({})

             @mock_filesystem.stubs(:all_files).returns([
                                                          { file: mock_file1, path: 'main.rb' },
                                                          { file: mock_file2, path: 'main.js' }
                                                        ])

             result = @list_tool.call(pattern: '*.rb')

             assert result[:success]
             assert_equal 1, result[:count]
             assert_equal 'main.rb', result[:files][0][:path]
           end

           def test_list_files_with_complex_pattern
             mock_file1 = mock('file1')
             mock_file1.stubs(:name).returns('main.rb')
             mock_file1.stubs(:content).returns('content')
             mock_file1.stubs(:attributes).returns({})

             mock_file2 = mock('file2')
             mock_file2.stubs(:name).returns('test.rb')
             mock_file2.stubs(:content).returns('content')
             mock_file2.stubs(:attributes).returns({})

             mock_file3 = mock('file3')
             mock_file3.stubs(:name).returns('main.js')
             mock_file3.stubs(:content).returns('content')
             mock_file3.stubs(:attributes).returns({})

             @mock_filesystem.stubs(:all_files).returns([
                                                          { file: mock_file1, path: 'src/main.rb' },
                                                          { file: mock_file2, path: 'test/main.rb' },
                                                          { file: mock_file3, path: 'src/main.js' }
                                                        ])

             result = @list_tool.call(pattern: '{src,test}/main.rb')

             assert result[:success]
             assert_equal 2, result[:count]
             paths = result[:files].map { |f| f[:path] }
             assert_includes paths, 'src/main.rb'
             assert_includes paths, 'test/main.rb'
             refute_includes paths, 'src/main.js'
           end

           def test_list_files_with_special_characters_in_path
             mock_file = mock('file')
             mock_file.stubs(:name).returns('file-name_with.special.chars.txt')
             mock_file.stubs(:content).returns('content')
             mock_file.stubs(:attributes).returns({})

             @mock_filesystem.stubs(:all_files).returns([
                                                          { file: mock_file, path: 'dir/file-name_with.special.chars.txt' }
                                                        ])

             result = @list_tool.call(pattern: 'dir/file-name_with.special.chars.txt')

             assert result[:success]
             assert_equal 1, result[:count]
             assert_equal 'dir/file-name_with.special.chars.txt', result[:files][0][:path]
           end

           def test_list_files_with_binary_content
             mock_file = mock('file')
             binary_content = "text\x00binary\x01data"
             mock_file.stubs(:name).returns('binary.dat')
             mock_file.stubs(:content).returns(binary_content)
             mock_file.stubs(:attributes).returns({ type: 'binary' })

             @mock_filesystem.stubs(:all_files).returns([
                                                          { file: mock_file, path: 'binary.dat' }
                                                        ])

             result = @list_tool.call

             assert result[:success]
             assert_equal 1, result[:count]
             file_info = result[:files][0]
             assert_equal 'binary.dat', file_info[:path]
             assert_equal binary_content.bytesize, file_info[:size]
             assert_equal({ type: 'binary' }, file_info[:attributes])
           end

           def test_list_files_with_large_content
             large_content = 'a' * 1000000
             mock_file = mock('file')
             mock_file.stubs(:name).returns('large.txt')
             mock_file.stubs(:content).returns(large_content)
             mock_file.stubs(:attributes).returns({})

             @mock_filesystem.stubs(:all_files).returns([
                                                          { file: mock_file, path: 'large.txt' }
                                                        ])

             result = @list_tool.call

             assert result[:success]
             assert_equal 1, result[:count]
             assert_equal 1000000, result[:files][0][:size]
           end

           def test_list_files_with_unicode_content
             unicode_content = 'Hello 世界 🌍'
             mock_file = mock('file')
             mock_file.stubs(:name).returns('unicode.txt')
             mock_file.stubs(:content).returns(unicode_content)
             mock_file.stubs(:attributes).returns({})

             @mock_filesystem.stubs(:all_files).returns([
                                                          { file: mock_file, path: 'unicode.txt' }
                                                        ])

             result = @list_tool.call

             assert result[:success]
             assert_equal 1, result[:count]
             assert_equal unicode_content.bytesize, result[:files][0][:size]
           end

           def test_list_files_with_empty_content
             mock_file = mock('file')
             mock_file.stubs(:name).returns('empty.txt')
             mock_file.stubs(:content).returns('')
             mock_file.stubs(:attributes).returns({})

             @mock_filesystem.stubs(:all_files).returns([
                                                          { file: mock_file, path: 'empty.txt' }
                                                        ])

             result = @list_tool.call

             assert result[:success]
             assert_equal 1, result[:count]
             assert_equal 0, result[:files][0][:size]
           end

           def test_list_files_with_various_attributes
             mock_file = mock('file')
             mock_file.stubs(:name).returns('file.txt')
             mock_file.stubs(:content).returns('content')
             mock_file.stubs(:attributes).returns({
                                                    type: 'text',
                                                    encoding: 'utf-8',
                                                    permissions: 'rw-r--r--',
                                                    modified: Time.now.to_s
                                                  })

             @mock_filesystem.stubs(:all_files).returns([
                                                          { file: mock_file, path: 'file.txt' }
                                                        ])

             result = @list_tool.call

             assert result[:success]
             assert_equal 1, result[:count]
             file_info = result[:files][0]
             expected_attrs = {
                               type: 'text',
                               encoding: 'utf-8',
                               permissions: 'rw-r--r--',
                               modified: Time.now.to_s
                             }
             assert_equal expected_attrs, file_info[:attributes]
           end

           def test_list_files_with_many_files
             files = 100.times.map do |i|
               mock_file = mock("file#{i}")
               mock_file.stubs(:name).returns("file#{i}.txt")
               mock_file.stubs(:content).returns('content')
               mock_file.stubs(:attributes).returns({})
               { file: mock_file, path: "file#{i}.txt" }
             end

             @mock_filesystem.stubs(:all_files).returns(files)

             result = @list_tool.call

             assert result[:success]
             assert_equal 100, result[:count]
             assert_equal 100, result[:files].length
           end

           def test_list_files_with_invalid_pattern_does_not_crash
             mock_file = mock('file')
             mock_file.stubs(:name).returns('file.txt')
             mock_file.stubs(:content).returns('content')
             mock_file.stubs(:attributes).returns({})

             @mock_filesystem.stubs(:all_files).returns([
                                                          { file: mock_file, path: 'file.txt' }
                                                        ])

             # Invalid patterns should not match anything or raise exceptions
             result = @list_tool.call(pattern: '[invalid')

             assert result[:success]
             # Depending on fnmatch behavior, it might match nothing or treat literally
             # Just ensure it doesn't crash
           end

           def test_list_files_pattern_with_spaces
             mock_file = mock('file')
             mock_file.stubs(:name).returns('my file.txt')
             mock_file.stubs(:content).returns('content')
             mock_file.stubs(:attributes).returns({})

             @mock_filesystem.stubs(:all_files).returns([
                                                          { file: mock_file, path: 'my file.txt' }
                                                        ])

             result = @list_tool.call(pattern: 'my file.txt')

             assert result[:success]
             assert_equal 1, result[:count]
             assert_equal 'my file.txt', result[:files][0][:path]
           end
         end

      class GlobTest < Minitest::Test
          def setup
            @mock_filesystem = mock('filesystem')
            @glob_tool = Cosmos::Llm::Tool::Preset.glob(@mock_filesystem)
          end

          def test_glob_tool_creation
            assert_kind_of Cosmos::Llm::Tool::MockToolDefinition, @glob_tool
          end

          def test_glob_tool_schema
            schema = @glob_tool.to_openai_schema
            assert_equal 'glob', schema['function']['name']
            assert_includes schema['function']['description'], 'Find files matching glob patterns'
            assert_includes schema['function']['parameters']['properties'], 'pattern'
          end

          def test_glob_simple_pattern
            @mock_filesystem.stubs(:all_files).returns([
                                                         { path: 'main.rb' },
                                                         { path: 'main.js' },
                                                         { path: 'helper.rb' }
                                                       ])

            result = @glob_tool.call(pattern: '*.rb')

            assert result[:success]
            assert_equal '*.rb', result[:pattern]
            assert_equal 2, result[:count]
            assert_includes result[:paths], 'main.rb'
            assert_includes result[:paths], 'helper.rb'
            refute_includes result[:paths], 'main.js'
          end

          def test_glob_directory_pattern
            @mock_filesystem.stubs(:all_files).returns([
                                                         { path: 'src/main.rb' },
                                                         { path: 'lib/helper.rb' },
                                                         { path: 'main.js' }
                                                       ])

            result = @glob_tool.call(pattern: 'src/*.rb')

            assert result[:success]
            assert_equal 1, result[:count]
            assert_includes result[:paths], 'src/main.rb'
            refute_includes result[:paths], 'lib/helper.rb'
            refute_includes result[:paths], 'main.js'
          end

          def test_glob_question_mark
            @mock_filesystem.stubs(:all_files).returns([
                                                         { path: 'file1.txt' },
                                                         { path: 'file2.txt' },
                                                         { path: 'file10.txt' }
                                                       ])

            result = @glob_tool.call(pattern: 'file?.txt')

            assert result[:success]
            assert_equal 2, result[:count]
            assert_includes result[:paths], 'file1.txt'
            assert_includes result[:paths], 'file2.txt'
            refute_includes result[:paths], 'file10.txt'
          end

          def test_glob_alternation
            @mock_filesystem.stubs(:all_files).returns([
                                                         { path: 'main.rb' },
                                                         { path: 'main.js' },
                                                         { path: 'main.py' }
                                                       ])

            result = @glob_tool.call(pattern: 'main.{rb,js}')

            assert result[:success]
            assert_equal 2, result[:count]
            assert_includes result[:paths], 'main.rb'
            assert_includes result[:paths], 'main.js'
            refute_includes result[:paths], 'main.py'
          end

          def test_glob_no_matches
            @mock_filesystem.stubs(:all_files).returns([
                                                         { path: 'main.js' },
                                                         { path: 'helper.js' }
                                                       ])

            result = @glob_tool.call(pattern: '*.rb')

            assert result[:success]
            assert_equal 0, result[:count]
            assert_empty result[:paths]
          end

          def test_glob_invalid_pattern
            @mock_filesystem.stubs(:all_files).returns([])

            result = @glob_tool.call(pattern: '**/*')

            # This should succeed as it's a valid pattern
            assert result[:success]
          end

           def test_glob_exception_handling
             @mock_filesystem.stubs(:all_files).raises(StandardError.new('filesystem error'))

             result = @glob_tool.call(pattern: '*.rb')

             refute result[:success]
             assert_equal 'filesystem error', result[:error]
           end

           def test_glob_double_star_recursive
             @mock_filesystem.stubs(:all_files).returns([
                                                          { path: 'file.rb' },
                                                          { path: 'src/file.rb' },
                                                          { path: 'src/sub/file.rb' },
                                                          { path: 'src/sub/deep/file.rb' },
                                                          { path: 'other/file.js' }
                                                        ])

             result = @glob_tool.call(pattern: '**/*.rb')

             assert result[:success]
             assert_equal 4, result[:count]
             assert_includes result[:paths], 'file.rb'
             assert_includes result[:paths], 'src/file.rb'
             assert_includes result[:paths], 'src/sub/file.rb'
             assert_includes result[:paths], 'src/sub/deep/file.rb'
             refute_includes result[:paths], 'other/file.js'
           end

           def test_glob_double_star_at_end
             @mock_filesystem.stubs(:all_files).returns([
                                                          { path: 'src' },
                                                          { path: 'src/file.rb' },
                                                          { path: 'src/sub' },
                                                          { path: 'src/sub/file.rb' }
                                                        ])

             result = @glob_tool.call(pattern: 'src/**')

             assert result[:success]
             assert_equal 4, result[:count]
             assert_includes result[:paths], 'src'
             assert_includes result[:paths], 'src/file.rb'
             assert_includes result[:paths], 'src/sub'
             assert_includes result[:paths], 'src/sub/file.rb'
           end

           def test_glob_alternation_with_spaces
             @mock_filesystem.stubs(:all_files).returns([
                                                          { path: 'main.rb' },
                                                          { path: 'main.js' },
                                                          { path: 'main.py' }
                                                        ])

             result = @glob_tool.call(pattern: 'main.{rb, js}')

             assert result[:success]
             assert_equal 2, result[:count]
             assert_includes result[:paths], 'main.rb'
             assert_includes result[:paths], 'main.js'
             refute_includes result[:paths], 'main.py'
           end

           def test_glob_multiple_alternations
             @mock_filesystem.stubs(:all_files).returns([
                                                          { path: 'file.rb' },
                                                          { path: 'file.js' },
                                                          { path: 'test.rb' },
                                                          { path: 'test.js' },
                                                          { path: 'file.py' }
                                                        ])

             result = @glob_tool.call(pattern: '{file,test}.{rb,js}')

             assert result[:success]
             assert_equal 4, result[:count]
             assert_includes result[:paths], 'file.rb'
             assert_includes result[:paths], 'file.js'
             assert_includes result[:paths], 'test.rb'
             assert_includes result[:paths], 'test.js'
             refute_includes result[:paths], 'file.py'
           end

           def test_glob_literal_dot
             @mock_filesystem.stubs(:all_files).returns([
                                                          { path: 'file.rb' },
                                                          { path: 'file.js' },
                                                          { path: 'test.rb' }
                                                        ])

             result = @glob_tool.call(pattern: '*.rb')

             assert result[:success]
             assert_equal 2, result[:count]
             assert_includes result[:paths], 'file.rb'
             assert_includes result[:paths], 'test.rb'
             refute_includes result[:paths], 'file.js'
           end

           def test_glob_empty_pattern
             @mock_filesystem.stubs(:all_files).returns([
                                                          { path: 'file.rb' }
                                                        ])

             result = @glob_tool.call(pattern: '')

             assert result[:success]
             assert_equal 0, result[:count]
             assert_empty result[:paths]
           end

           def test_glob_single_question_mark
             @mock_filesystem.stubs(:all_files).returns([
                                                          { path: 'file1.txt' },
                                                          { path: 'file2.txt' },
                                                          { path: 'file10.txt' },
                                                          { path: 'file.txt' }
                                                        ])

             result = @glob_tool.call(pattern: 'file?.txt')

             assert result[:success]
             assert_equal 3, result[:count]
             assert_includes result[:paths], 'file1.txt'
             assert_includes result[:paths], 'file2.txt'
             assert_includes result[:paths], 'file.txt'
             refute_includes result[:paths], 'file10.txt'
           end

           def test_glob_complex_pattern
             @mock_filesystem.stubs(:all_files).returns([
                                                          { path: 'src/main.rb' },
                                                          { path: 'src/main.js' },
                                                          { path: 'src/test.rb' },
                                                          { path: 'lib/main.rb' },
                                                          { path: 'src/utils/helper.rb' }
                                                        ])

             result = @glob_tool.call(pattern: 'src/**/*.rb')

             assert result[:success]
             assert_equal 3, result[:count]
             assert_includes result[:paths], 'src/main.rb'
             assert_includes result[:paths], 'src/test.rb'
             assert_includes result[:paths], 'src/utils/helper.rb'
             refute_includes result[:paths], 'src/main.js'
             refute_includes result[:paths], 'lib/main.rb'
           end

           def test_glob_root_level_double_star
             @mock_filesystem.stubs(:all_files).returns([
                                                          { path: 'main.rb' },
                                                          { path: 'src/main.rb' },
                                                          { path: 'src/sub/main.rb' }
                                                        ])

             result = @glob_tool.call(pattern: '**/*.rb')

             assert result[:success]
             assert_equal 3, result[:count]
             assert_includes result[:paths], 'main.rb'
             assert_includes result[:paths], 'src/main.rb'
             assert_includes result[:paths], 'src/sub/main.rb'
           end

           def test_glob_no_directory_separator
             @mock_filesystem.stubs(:all_files).returns([
                                                          { path: 'main.rb' },
                                                          { path: 'main.js' }
                                                        ])

             result = @glob_tool.call(pattern: 'main.*')

             assert result[:success]
             assert_equal 2, result[:count]
             assert_includes result[:paths], 'main.rb'
             assert_includes result[:paths], 'main.js'
           end
        end

         class JqTest < Minitest::Test
           def setup
             @mock_filesystem = mock('filesystem')
             @jq_tool = Cosmos::Llm::Tool::Preset.jq(@mock_filesystem)
           end

           def test_jq_tool_creation
            assert_kind_of Cosmos::Llm::Tool::MockToolDefinition, @jq_tool
          end

          def test_jq_tool_schema
            schema = @jq_tool.to_openai_schema
            assert_equal 'jq', schema['function']['name']
            assert_includes schema['function']['description'], 'Query and transform JSON'
            assert_includes schema['function']['parameters']['properties'], 'json'
            assert_includes schema['function']['parameters']['properties'], 'file_path'
            assert_includes schema['function']['parameters']['properties'], 'query'
            assert_includes schema['function']['parameters']['properties'], 'compact'
          end

          def test_jq_identity_query
            json = '{"name":"Alice","age":30}'
            result = @jq_tool.call(json: json, query: '.')

            assert result[:success]
            assert_equal '.', result[:query]
            assert_equal({ 'name' => 'Alice', 'age' => 30 }, result[:result])
            assert_equal 'string', result[:source]
          end

          def test_jq_object_key_access
            json = '{"name":"Alice","age":30}'
            result = @jq_tool.call(json: json, query: '.name')

            assert result[:success]
            assert_equal 'Alice', result[:result]
          end

          def test_jq_array_index_access
            json = '{"items":[1,2,3,4]}'
            result = @jq_tool.call(json: json, query: '.items[0]')

            assert result[:success]
            assert_equal 1, result[:result]
          end

          def test_jq_array_iteration
            json = '{"items":[1,2,3]}'
            result = @jq_tool.call(json: json, query: '.items[]')

            assert result[:success]
            assert_equal [1, 2, 3], result[:result]
          end

          def test_jq_keys_query
            json = '{"a":1,"b":2}'
            result = @jq_tool.call(json: json, query: 'keys')

            assert result[:success]
            assert_equal %w[a b], result[:result].sort
          end

          def test_jq_length_query
            json = '[1,2,3,4]'
            result = @jq_tool.call(json: json, query: 'length')

            assert result[:success]
            assert_equal 4, result[:result]
          end

          def test_jq_compact_output
            json = '{"name":"Alice"}'
            result = @jq_tool.call(json: json, query: '.', compact: true)

            assert result[:success]
            assert_equal '{"name":"Alice"}', result[:output]
          end

          def test_jq_from_file
            mock_file = mock('file')
            json_content = '{"name":"Bob"}'
            mock_file.stubs(:content).returns(json_content)

            @mock_filesystem.stubs(:find_file).with('data.json').returns(mock_file)

            result = @jq_tool.call(file_path: 'data.json', query: '.name')

            assert result[:success]
            assert_equal 'Bob', result[:result]
            assert_equal 'file:data.json', result[:source]
          end

          def test_jq_file_not_found
            @mock_filesystem.stubs(:find_file).with('missing.json').returns(nil)

            result = @jq_tool.call(file_path: 'missing.json', query: '.')

            refute result[:success]
            assert_equal 'File not found in virtual filesystem', result[:error]
          end

          def test_jq_invalid_json
            result = @jq_tool.call(json: 'invalid json', query: '.')

            refute result[:success]
            assert_includes result[:error], 'Invalid JSON'
          end

          def test_jq_no_input
            result = @jq_tool.call(query: '.')

            refute result[:success]
            assert_equal 'Either json or file_path parameter is required', result[:error]
          end

          def test_jq_invalid_query
            json = '{"name":"Alice"}'
            result = @jq_tool.call(json: json, query: '.nonexistent')

            assert result[:success]
            assert_nil result[:result]
          end

           def test_jq_exception_handling
             @mock_filesystem.stubs(:find_file).raises(StandardError.new('filesystem error'))

             result = @jq_tool.call(file_path: 'test.json', query: '.')

             refute result[:success]
             assert_equal 'filesystem error', result[:error]
           end

           def test_jq_values_query_object
             json = '{"a":1,"b":2,"c":3}'
             result = @jq_tool.call(json: json, query: 'values')

             assert result[:success]
             assert_equal [1, 2, 3], result[:result].sort
           end

           def test_jq_values_query_array
             json = '[1,2,3,4]'
             result = @jq_tool.call(json: json, query: 'values')

             assert result[:success]
             assert_equal [1, 2, 3, 4], result[:result]
           end

           def test_jq_type_query_object
             json = '{"name":"Alice"}'
             result = @jq_tool.call(json: json, query: 'type')

             assert result[:success]
             assert_equal 'object', result[:result]
           end

           def test_jq_type_query_array
             json = '[1,2,3]'
             result = @jq_tool.call(json: json, query: 'type')

             assert result[:success]
             assert_equal 'array', result[:result]
           end

           def test_jq_type_query_string
             json = '"hello"'
             result = @jq_tool.call(json: json, query: 'type')

             assert result[:success]
             assert_equal 'string', result[:result]
           end

           def test_jq_type_query_number
             json = '42'
             result = @jq_tool.call(json: json, query: 'type')

             assert result[:success]
             assert_equal 'number', result[:result]
           end

           def test_jq_type_query_boolean
             json = 'true'
             result = @jq_tool.call(json: json, query: 'type')

             assert result[:success]
             assert_equal 'boolean', result[:result]
           end

           def test_jq_type_query_null
             json = 'null'
             result = @jq_tool.call(json: json, query: 'type')

             assert result[:success]
             assert_equal 'null', result[:result]
           end

           def test_jq_nested_object_access
             json = '{"user":{"name":"Alice","age":30}}'
             result = @jq_tool.call(json: json, query: '.user.name')

             assert result[:success]
             assert_equal 'Alice', result[:result]
           end

           def test_jq_array_element_access
             json = '{"items":[{"name":"item1"},{"name":"item2"}]}'
             result = @jq_tool.call(json: json, query: '.items[0].name')

             assert result[:success]
             assert_equal 'item1', result[:result]
           end

           def test_jq_negative_array_index
             json = '{"items":[1,2,3,4]}'
             result = @jq_tool.call(json: json, query: '.items[-1]')

             assert result[:success]
             assert_equal 4, result[:result]
           end

           def test_jq_array_out_of_bounds
             json = '{"items":[1,2,3]}'
             result = @jq_tool.call(json: json, query: '.items[10]')

             assert result[:success]
             assert_nil result[:result]
           end

           def test_jq_invalid_array_index
             json = '{"items":[1,2,3]}'
             result = @jq_tool.call(json: json, query: '.items[abc]')

             refute result[:success]
             assert_includes result[:error], 'Invalid array index'
           end

           def test_jq_access_nonexistent_key
             json = '{"name":"Alice"}'
             result = @jq_tool.call(json: json, query: '.age')

             assert result[:success]
             assert_nil result[:result]
           end

           def test_jq_access_key_on_array
             json = '[1,2,3]'
             result = @jq_tool.call(json: json, query: '.name')

             refute result[:success]
             assert_includes result[:error], 'Cannot access key'
           end

           def test_jq_index_non_array
             json = '{"name":"Alice"}'
             result = @jq_tool.call(json: json, query: '.name[0]')

             refute result[:success]
             assert_includes result[:error], 'Cannot index non-array'
           end

           def test_jq_iterate_non_array
             json = '{"name":"Alice"}'
             result = @jq_tool.call(json: json, query: '.name[]')

             refute result[:success]
             assert_includes result[:error], 'Cannot iterate over non-array'
           end

           def test_jq_keys_on_non_object_or_array
             json = '"string"'
             result = @jq_tool.call(json: json, query: 'keys')

             refute result[:success]
             assert_includes result[:error], 'Cannot get keys of'
           end

           def test_jq_values_on_non_object_or_array
             json = '42'
             result = @jq_tool.call(json: json, query: 'values')

             refute result[:success]
             assert_includes result[:error], 'Cannot get values of'
           end

           def test_jq_length_on_non_respondable
             json = 'null'
             result = @jq_tool.call(json: json, query: 'length')

             refute result[:success]
             assert_includes result[:error], 'Cannot get length of'
           end

           def test_jq_complex_nested_query
             json = '{"users":[{"profile":{"name":"Alice","hobbies":["reading","coding"]}},{"profile":{"name":"Bob","hobbies":["gaming"]}}]}'
             result = @jq_tool.call(json: json, query: '.users[0].profile.hobbies[1]')

             assert result[:success]
             assert_equal 'coding', result[:result]
           end

           def test_jq_pretty_output_default
             json = '{"name":"Alice","age":30}'
             result = @jq_tool.call(json: json, query: '.')

             assert result[:success]
             assert_includes result[:output], "\n"
             assert_includes result[:output], '  "name": "Alice"'
           end

           def test_jq_compact_output_explicit_false
             json = '{"name":"Alice"}'
             result = @jq_tool.call(json: json, query: '.', compact: false)

             assert result[:success]
             assert_includes result[:output], "\n"
           end

           def test_jq_empty_array_iteration
             json = '{"items":[]}'
             result = @jq_tool.call(json: json, query: '.items[]')

             assert result[:success]
             assert_equal [], result[:result]
           end

           def test_jq_single_element_array_iteration
             json = '{"items":[42]}'
             result = @jq_tool.call(json: json, query: '.items[]')

             assert result[:success]
             assert_equal [42], result[:result]
           end

           def test_jq_keys_sorted
             json = '{"z":1,"a":2,"m":3}'
             result = @jq_tool.call(json: json, query: 'keys')

             assert result[:success]
             assert_equal %w[a m z], result[:result]
           end

           def test_jq_values_sorted_by_keys
             json = '{"z":1,"a":2,"m":3}'
             result = @jq_tool.call(json: json, query: 'values')

             assert result[:success]
             assert_equal [2, 3, 1], result[:result] # values in key order: a, m, z
           end

           def test_jq_array_keys
             json = '[10,20,30]'
             result = @jq_tool.call(json: json, query: 'keys')

             assert result[:success]
             assert_equal [0, 1, 2], result[:result]
           end

           def test_jq_empty_object_keys
             json = '{}'
             result = @jq_tool.call(json: json, query: 'keys')

             assert result[:success]
             assert_equal [], result[:result]
           end

           def test_jq_empty_array_keys
             json = '[]'
             result = @jq_tool.call(json: json, query: 'keys')

             assert result[:success]
             assert_equal [], result[:result]
           end

           def test_jq_length_object
             json = '{"a":1,"b":2,"c":3}'
             result = @jq_tool.call(json: json, query: 'length')

             assert result[:success]
             assert_equal 3, result[:result]
           end

           def test_jq_length_string
             json = '"hello world"'
             result = @jq_tool.call(json: json, query: 'length')

             assert result[:success]
             assert_equal 11, result[:result]
           end

           def test_jq_length_empty_string
             json = '""'
             result = @jq_tool.call(json: json, query: 'length')

             assert result[:success]
             assert_equal 0, result[:result]
           end

           def test_jq_length_empty_array
             json = '[]'
             result = @jq_tool.call(json: json, query: 'length')

             assert result[:success]
             assert_equal 0, result[:result]
           end

           def test_jq_length_empty_object
             json = '{}'
             result = @jq_tool.call(json: json, query: 'length')

             assert result[:success]
             assert_equal 0, result[:result]
           end

           def test_jq_no_leading_dot
             json = '{"name":"Alice"}'
             result = @jq_tool.call(json: json, query: 'name')

             assert result[:success]
             assert_equal 'Alice', result[:result]
           end

           def test_jq_deeply_nested_path
             json = '{"a":{"b":{"c":{"d":"value"}}}}'
             result = @jq_tool.call(json: json, query: '.a.b.c.d')

             assert result[:success]
             assert_equal 'value', result[:result]
           end

           def test_jq_mixed_array_and_object_access
             json = '{"data":[{"info":{"count":5}},{"info":{"count":10}}]}'
             result = @jq_tool.call(json: json, query: '.data[1].info.count')

             assert result[:success]
             assert_equal 10, result[:result]
           end

           def test_jq_invalid_query_syntax
             json = '{"name":"Alice"}'
             result = @jq_tool.call(json: json, query: '.name[')

             refute result[:success]
             assert_includes result[:error], 'StandardError'
           end

           def test_jq_query_with_whitespace
             json = '{"name":"Alice"}'
             result = @jq_tool.call(json: json, query: ' .name ')

             assert result[:success]
             assert_equal 'Alice', result[:result]
           end

           def test_jq_float_number_type
             json = '3.14'
             result = @jq_tool.call(json: json, query: 'type')

             assert result[:success]
             assert_equal 'number', result[:result]
           end

           def test_jq_zero_number
             json = '0'
             result = @jq_tool.call(json: json, query: '.')

             assert result[:success]
             assert_equal 0, result[:result]
           end

           def test_jq_false_boolean
             json = 'false'
             result = @jq_tool.call(json: json, query: '.')

             assert result[:success]
             assert_equal false, result[:result]
           end
          end

        class WebfetchTest < Minitest::Test
           def setup
             @webfetch_tool = Cosmos::Llm::Tool::Preset.webfetch
           end

           def test_webfetch_tool_creation
             assert_kind_of Cosmos::Llm::Tool::MockToolDefinition, @webfetch_tool
           end

           def test_webfetch_tool_schema
             schema = @webfetch_tool.to_openai_schema
             assert_equal 'webfetch', schema['function']['name']
             assert_includes schema['function']['description'], 'Fetch content from URLs'
             assert_includes schema['function']['parameters']['properties'], 'url'
             assert_includes schema['function']['parameters']['properties'], 'format'
             assert_includes schema['function']['parameters']['properties'], 'timeout'
           end

           def test_webfetch_successful_html_fetch
             html_content = '<html><body><h1>Test</h1><p>Content</p></body></html>'
             mock_response = mock('response')
             mock_response.stubs(:body).returns(html_content)
             mock_response.stubs(:[]).with('content-type').returns('text/html')
             mock_response.stubs(:code).returns('200')

             mock_http = mock('http')
             mock_http.stubs(:use_ssl=)
             mock_http.stubs(:open_timeout=)
             mock_http.stubs(:read_timeout=)
             mock_http.stubs(:request).returns(mock_response)

             Net::HTTP.stubs(:new).returns(mock_http)

             result = @webfetch_tool.call(url: 'https://example.com')

             assert result[:success]
             assert_equal 'https://example.com', result[:url]
             assert_equal 'markdown', result[:format]
             assert_includes result[:content], '# Test'
             assert_includes result[:content], 'Content'
             assert_equal 'text/html', result[:content_type]
             assert result[:size] > 0
             assert result[:fetched_at]
           end

           def test_webfetch_successful_text_format
             html_content = '<html><body><h1>Test</h1><p>Content</p></body></html>'
             mock_response = mock('response')
             mock_response.stubs(:body).returns(html_content)
             mock_response.stubs(:[]).with('content-type').returns('text/html')

             mock_http = mock('http')
             mock_http.stubs(:use_ssl=)
             mock_http.stubs(:open_timeout=)
             mock_http.stubs(:read_timeout=)
             mock_http.stubs(:request).returns(mock_response)

             Net::HTTP.stubs(:new).returns(mock_http)

             result = @webfetch_tool.call(url: 'https://example.com', format: 'text')

             assert result[:success]
             assert_equal 'text', result[:format]
             assert_includes result[:content], 'Test'
             assert_includes result[:content], 'Content'
             refute_includes result[:content], '# ' # No markdown headers
           end

           def test_webfetch_successful_html_format
             html_content = '<html><body><h1>Test</h1></body></html>'
             mock_response = mock('response')
             mock_response.stubs(:body).returns(html_content)
             mock_response.stubs(:[]).with('content-type').returns('text/html')

             mock_http = mock('http')
             mock_http.stubs(:use_ssl=)
             mock_http.stubs(:open_timeout=)
             mock_http.stubs(:read_timeout=)
             mock_http.stubs(:request).returns(mock_response)

             Net::HTTP.stubs(:new).returns(mock_http)

             result = @webfetch_tool.call(url: 'https://example.com', format: 'html')

             assert result[:success]
             assert_equal 'html', result[:format]
             assert_equal html_content, result[:content]
           end

           def test_webfetch_http_upgrade_to_https
             html_content = '<html><body>Test</body></html>'
             mock_response = mock('response')
             mock_response.stubs(:body).returns(html_content)
             mock_response.stubs(:[]).with('content-type').returns('text/html')

             mock_http = mock('http')
             mock_http.stubs(:use_ssl=).with(true) # Should set SSL for HTTPS
             mock_http.stubs(:open_timeout=)
             mock_http.stubs(:read_timeout=)
             mock_http.stubs(:request).returns(mock_response)

             Net::HTTP.stubs(:new).with('example.com', 443).returns(mock_http)

             result = @webfetch_tool.call(url: 'http://example.com')

             assert result[:success]
             assert_equal 'https://example.com', result[:url] # Should be upgraded
           end

           def test_webfetch_with_redirect
             html_content = '<html><body>Final content</body></html>'

             # First response: redirect
             redirect_response = mock('redirect_response')
             redirect_response.stubs(:is_a?).with(Net::HTTPSuccess).returns(false)
             redirect_response.stubs(:is_a?).with(Net::HTTPRedirection).returns(true)
             redirect_response.stubs(:[]).with('location').returns('https://example.com/final')
             redirect_response.stubs(:body).returns('redirect')

             # Second response: success
             success_response = mock('success_response')
             success_response.stubs(:is_a?).with(Net::HTTPSuccess).returns(true)
             success_response.stubs(:is_a?).with(Net::HTTPRedirection).returns(false)
             success_response.stubs(:body).returns(html_content)
             success_response.stubs(:[]).with('content-type').returns('text/html')

             mock_http1 = mock('http1')
             mock_http1.stubs(:use_ssl=)
             mock_http1.stubs(:open_timeout=)
             mock_http1.stubs(:read_timeout=)
             mock_http1.stubs(:request).returns(redirect_response)

             mock_http2 = mock('http2')
             mock_http2.stubs(:use_ssl=)
             mock_http2.stubs(:open_timeout=)
             mock_http2.stubs(:read_timeout=)
             mock_http2.stubs(:request).returns(success_response)

             Net::HTTP.stubs(:new).with('example.com', 443).returns(mock_http1)
             Net::HTTP.stubs(:new).with('example.com', 443).returns(mock_http2)

             result = @webfetch_tool.call(url: 'https://example.com/redirect')

             assert result[:success]
             assert_equal 'https://example.com/final', result[:url]
           end

           def test_webfetch_max_redirects_exceeded
             # Create 6 redirect responses (more than max 5)
             redirect_responses = 6.times.map do |i|
               resp = mock("redirect_response_#{i}")
               resp.stubs(:is_a?).with(Net::HTTPSuccess).returns(false)
               resp.stubs(:is_a?).with(Net::HTTPRedirection).returns(true)
               resp.stubs(:[]).with('location').returns("https://example.com/#{i + 1}")
               resp.stubs(:body).returns('redirect')
               resp
             end

             mock_https = redirect_responses.map do |resp|
               http = mock('http')
               http.stubs(:use_ssl=)
               http.stubs(:open_timeout=)
               http.stubs(:read_timeout=)
               http.stubs(:request).returns(resp)
               http
             end

             Net::HTTP.stubs(:new).returns(*mock_https)

             result = @webfetch_tool.call(url: 'https://example.com/start')

             refute result[:success]
             assert_equal 'Failed to fetch content', result[:error]
           end

           def test_webfetch_invalid_url
             result = @webfetch_tool.call(url: 'not-a-url')

             refute result[:success]
             assert_includes result[:error], 'Invalid URL'
           end

           def test_webfetch_non_http_url
             result = @webfetch_tool.call(url: 'ftp://example.com')

             refute result[:success]
             assert_includes result[:error], 'Invalid HTTP URL'
           end

           def test_webfetch_timeout
             mock_http = mock('http')
             mock_http.stubs(:use_ssl=)
             mock_http.stubs(:open_timeout=)
             mock_http.stubs(:read_timeout=)
             mock_http.stubs(:request).raises(Net::ReadTimeout.new('timeout'))

             Net::HTTP.stubs(:new).returns(mock_http)

             result = @webfetch_tool.call(url: 'https://example.com')

             refute result[:success]
             assert_includes result[:error], 'Request timeout'
           end

           def test_webfetch_network_error
             mock_http = mock('http')
             mock_http.stubs(:use_ssl=)
             mock_http.stubs(:open_timeout=)
             mock_http.stubs(:read_timeout=)
             mock_http.stubs(:request).raises(StandardError.new('network error'))

             Net::HTTP.stubs(:new).returns(mock_http)

             result = @webfetch_tool.call(url: 'https://example.com')

             refute result[:success]
             assert_equal 'network error', result[:error]
           end

           def test_webfetch_invalid_format
             result = @webfetch_tool.call(url: 'https://example.com', format: 'invalid')

             refute result[:success]
             assert_includes result[:error], 'Unsupported format'
           end

           def test_webfetch_timeout_validation
             html_content = '<html><body>Test</body></html>'
             mock_response = mock('response')
             mock_response.stubs(:body).returns(html_content)
             mock_response.stubs(:[]).with('content-type').returns('text/html')

             mock_http = mock('http')
             mock_http.stubs(:use_ssl=)
             mock_http.stubs(:open_timeout=).with(120) # Should be clamped to 120
             mock_http.stubs(:read_timeout=).with(120)
             mock_http.stubs(:request).returns(mock_response)

             Net::HTTP.stubs(:new).returns(mock_http)

             result = @webfetch_tool.call(url: 'https://example.com', timeout: 200)

             assert result[:success]
           end

           def test_webfetch_timeout_minimum
             html_content = '<html><body>Test</body></html>'
             mock_response = mock('response')
             mock_response.stubs(:body).returns(html_content)
             mock_response.stubs(:[]).with('content-type').returns('text/html')

             mock_http = mock('http')
             mock_http.stubs(:use_ssl=)
             mock_http.stubs(:open_timeout=).with(1) # Should be clamped to 1
             mock_http.stubs(:read_timeout=).with(1)
             mock_http.stubs(:request).returns(mock_response)

             Net::HTTP.stubs(:new).returns(mock_http)

             result = @webfetch_tool.call(url: 'https://example.com', timeout: 0)

             assert result[:success]
           end

           def test_webfetch_empty_redirect_location
             redirect_response = mock('redirect_response')
             redirect_response.stubs(:is_a?).with(Net::HTTPSuccess).returns(false)
             redirect_response.stubs(:is_a?).with(Net::HTTPRedirection).returns(true)
             redirect_response.stubs(:[]).with('location').returns('')
             redirect_response.stubs(:body).returns('redirect')

             mock_http = mock('http')
             mock_http.stubs(:use_ssl=)
             mock_http.stubs(:open_timeout=)
             mock_http.stubs(:read_timeout=)
             mock_http.stubs(:request).returns(redirect_response)

             Net::HTTP.stubs(:new).returns(mock_http)

             result = @webfetch_tool.call(url: 'https://example.com')

             refute result[:success]
             assert_equal 'Failed to fetch content', result[:error]
           end

           def test_webfetch_invalid_redirect_location
             redirect_response = mock('redirect_response')
             redirect_response.stubs(:is_a?).with(Net::HTTPSuccess).returns(false)
             redirect_response.stubs(:is_a?).with(Net::HTTPRedirection).returns(true)
             redirect_response.stubs(:[]).with('location').returns('::invalid::')
             redirect_response.stubs(:body).returns('redirect')

             mock_http = mock('http')
             mock_http.stubs(:use_ssl=)
             mock_http.stubs(:open_timeout=)
             mock_http.stubs(:read_timeout=)
             mock_http.stubs(:request).returns(redirect_response)

             Net::HTTP.stubs(:new).returns(mock_http)

             result = @webfetch_tool.call(url: 'https://example.com')

             refute result[:success]
             assert_equal 'Failed to fetch content', result[:error]
           end

           def test_webfetch_markdown_conversion_with_lists
             html_content = '<html><body><h1>Title</h1><ul><li>Item 1</li><li>Item 2</li></ul></body></html>'
             mock_response = mock('response')
             mock_response.stubs(:body).returns(html_content)
             mock_response.stubs(:[]).with('content-type').returns('text/html')

             mock_http = mock('http')
             mock_http.stubs(:use_ssl=)
             mock_http.stubs(:open_timeout=)
             mock_http.stubs(:read_timeout=)
             mock_http.stubs(:request).returns(mock_response)

             Net::HTTP.stubs(:new).returns(mock_http)

             result = @webfetch_tool.call(url: 'https://example.com', format: 'markdown')

             assert result[:success]
             assert_includes result[:content], '# Title'
             assert_includes result[:content], '- Item 1'
             assert_includes result[:content], '- Item 2'
           end

           def test_webfetch_markdown_conversion_with_links
             html_content = '<html><body><p>Check <a href="https://example.com">this link</a></p></body></html>'
             mock_response = mock('response')
             mock_response.stubs(:body).returns(html_content)
             mock_response.stubs(:[]).with('content-type').returns('text/html')

             mock_http = mock('http')
             mock_http.stubs(:use_ssl=)
             mock_http.stubs(:open_timeout=)
             mock_http.stubs(:read_timeout=)
             mock_http.stubs(:request).returns(mock_response)

             Net::HTTP.stubs(:new).returns(mock_http)

             result = @webfetch_tool.call(url: 'https://example.com', format: 'markdown')

             assert result[:success]
             assert_includes result[:content], '[this link](https://example.com)'
           end
         end
      end
  end
end
