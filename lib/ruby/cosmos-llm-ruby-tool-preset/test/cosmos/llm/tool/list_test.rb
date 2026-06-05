# frozen_string_literal: true

require 'minitest/autorun'
require 'mocha/minitest'

# Add lib to load path
$LOAD_PATH.unshift File.expand_path('../../../lib', __dir__)

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
          @execute_block&.call(params)
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

require_relative '../../../../lib/cosmos/llm/tool/preset'

module Cosmos
  module Llm
    module Tool
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
          assert_equal 2, result[:count]
          assert_equal 'src/**/*.rb', result[:pattern]
          paths = result[:files].map { |f| f[:path] }
          refute_includes paths, 'src/main.rb' # ** requires at least one subdirectory
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
                                                       { file: mock_file,
                                                         path: 'dir/file-name_with.special.chars.txt' }
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
          large_content = 'a' * 1_000_000
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
          assert_equal 1_000_000, result[:files][0][:size]
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
    end
  end
end
