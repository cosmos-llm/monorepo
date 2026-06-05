# frozen_string_literal: true

require 'test_helper'

module Cosmos
  module Llm
    module Context
      module BuilderMixins
        class ContentMethodsTest < Minitest::Test
          def setup
            @builder = Builder.new
          end

          # Tests for string method

          def test_string_with_valid_content
            result = @builder.string('Test content')

            assert_equal 1, @builder.blocks.length
            assert_equal :string, @builder.blocks.first.name
            assert_equal 'Test content', @builder.blocks.first.content
            assert_same @builder, result
          end

          def test_string_with_empty_string
            @builder.string('')

            assert_equal 1, @builder.blocks.length
            assert_equal '', @builder.blocks.first.content
          end

          def test_string_with_nil_content
            @builder.string(nil)

            assert_equal 1, @builder.blocks.length
            assert_nil @builder.blocks.first.content
          end

          def test_string_with_large_content
            large_content = 'a' * 10_000
            @builder.string(large_content)

            assert_equal large_content, @builder.blocks.first.content
          end

          def test_string_with_multiline_content
            multiline = "Line 1\nLine 2\nLine 3"
            @builder.string(multiline)

            assert_equal multiline, @builder.blocks.first.content
          end

          def test_string_with_unicode_content
            unicode = 'Hello 世界 🌍'
            @builder.string(unicode)

            assert_equal unicode, @builder.blocks.first.content
          end

          def test_string_chaining
            result = @builder.string('First').string('Second')

            assert_equal 2, @builder.blocks.length
            assert_equal 'First', @builder.blocks[0].content
            assert_equal 'Second', @builder.blocks[1].content
            assert_same @builder, result
          end

          # Tests for file_content method

          def test_file_content_with_valid_file
            require 'tempfile'
            file = Tempfile.new('test')
            file.write('File content')
            file.close

            @builder.file_content(file.path)

            assert_equal 1, @builder.blocks.length
            assert_equal File.basename(file.path).to_sym, @builder.blocks.first.name
            assert_equal 'File content', @builder.blocks.first.content

            file.unlink
          end

          def test_file_content_with_custom_name
            require 'tempfile'
            file = Tempfile.new('test')
            file.write('Content')
            file.close

            result = @builder.file_content(file.path, name: :custom)

            assert_equal :custom, @builder.blocks.first.name
            assert_same @builder, result

            file.unlink
          end

          def test_file_content_with_string_name
            require 'tempfile'
            file = Tempfile.new('test')
            file.write('Content')
            file.close

            @builder.file_content(file.path, name: 'string_name')

            assert_equal :string_name, @builder.blocks.first.name

            file.unlink
          end

          def test_file_content_with_nil_name
            require 'tempfile'
            file = Tempfile.new('test')
            file.write('Content')
            file.close

            @builder.file_content(file.path, name: nil)

            assert_equal File.basename(file.path).to_sym, @builder.blocks.first.name

            file.unlink
          end

          def test_file_content_with_empty_file
            require 'tempfile'
            file = Tempfile.new('test')
            file.close

            @builder.file_content(file.path)

            assert_equal '', @builder.blocks.first.content

            file.unlink
          end

          def test_file_content_with_large_file
            require 'tempfile'
            file = Tempfile.new('test')
            large_content = 'x' * 100_000
            file.write(large_content)
            file.close

            @builder.file_content(file.path)

            assert_equal large_content, @builder.blocks.first.content

            file.unlink
          end

          def test_file_content_with_unicode_file
            require 'tempfile'
            file = Tempfile.new('test')
            unicode_content = 'Unicode: 世界 🌍'
            file.write(unicode_content)
            file.close

            @builder.file_content(file.path)

            assert_equal unicode_content, @builder.blocks.first.content

            file.unlink
          end

          def test_file_content_chaining
            require 'tempfile'
            file1 = Tempfile.new('test1')
            file1.write('Content 1')
            file1.close

            file2 = Tempfile.new('test2')
            file2.write('Content 2')
            file2.close

            result = @builder.file_content(file1.path).file_content(file2.path)

            assert_equal 2, @builder.blocks.length
            assert_equal 'Content 1', @builder.blocks[0].content
            assert_equal 'Content 2', @builder.blocks[1].content
            assert_same @builder, result

            file1.unlink
            file2.unlink
          end

          # Tests for validate_file_path! private method

          def test_validate_file_path_with_nil
            assert_raises(ValidationError, 'File path cannot be nil') do
              @builder.send(:validate_file_path!, nil)
            end
          end

          def test_validate_file_path_with_empty_string
            assert_raises(ValidationError, 'File path cannot be empty') do
              @builder.send(:validate_file_path!, '')
            end
          end

          def test_validate_file_path_with_whitespace_only
            assert_raises(ValidationError, 'File path cannot be empty') do
              @builder.send(:validate_file_path!, '   ')
            end
          end

          def test_validate_file_path_with_non_string
            assert_raises(ValidationError, 'File path must be a String') do
              @builder.send(:validate_file_path!, 123)
            end
          end

          def test_validate_file_path_with_nonexistent_file
            assert_raises(Errno::ENOENT, 'File not found: /nonexistent/file.txt') do
              @builder.send(:validate_file_path!, '/nonexistent/file.txt')
            end
          end

          def test_validate_file_path_with_valid_file
            require 'tempfile'
            file = Tempfile.new('test')
            file.close

            assert_nil @builder.send(:validate_file_path!, file.path)

            file.unlink
          end

          # Edge cases

          def test_file_content_with_relative_path
            require 'tempfile'
            file = Tempfile.new('test')
            file.write('Relative path content')
            file.close

            # Use relative path
            relative_path = File.basename(file.path)
            Dir.chdir(File.dirname(file.path)) do
              @builder.file_content(relative_path)
              assert_equal 'Relative path content', @builder.blocks.first.content
            end

            file.unlink
          end

          def test_file_content_with_path_containing_spaces
            require 'tempfile'
            file = Tempfile.new(['test file', '.txt'])
            file.write('Content with spaces')
            file.close

            @builder.file_content(file.path)

            assert_equal 'Content with spaces', @builder.blocks.first.content

            file.unlink
          end

          def test_file_content_with_binary_file
            require 'tempfile'
            file = Tempfile.new('binary')
            binary_content = "\x00\x01\x02\x03\xFF\xFE\xFD"
            file.write(binary_content)
            file.close

            @builder.file_content(file.path)

            assert_equal binary_content, @builder.blocks.first.content

            file.unlink
          end

          def test_file_content_with_file_containing_newlines
            require 'tempfile'
            file = Tempfile.new('multiline')
            content = "Line 1\nLine 2\r\nLine 3\r"
            file.write(content)
            file.close

            @builder.file_content(file.path)

            assert_equal content, @builder.blocks.first.content

            file.unlink
          end

          # Error cases for file_content

          def test_file_content_with_nil_path
            assert_raises(ValidationError) do
              @builder.file_content(nil)
            end
          end

          def test_file_content_with_empty_path
            assert_raises(ValidationError) do
              @builder.file_content('')
            end
          end

          def test_file_content_with_non_string_path
            assert_raises(ValidationError) do
              @builder.file_content(123)
            end
          end

          def test_file_content_with_nonexistent_path
            assert_raises(Errno::ENOENT) do
              @builder.file_content('/definitely/does/not/exist.txt')
            end
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
