# frozen_string_literal: true

require 'test_helper'

module Cosmos
  module Llm
    module VirtualFilesystem
      class TestFilesystem < Minitest::Test
        def setup
          @fs = Filesystem.new('root')
        end

        def test_initialize
          assert_equal 'root', @fs.name
          assert_empty @fs.children
          assert_empty @fs.files
          assert_empty @fs.attributes
        end

        def test_initialize_with_block
          fs = Filesystem.new('project') do
            file 'README.md'
            directory 'src'
          end

          assert_equal 1, fs.files.length
          assert_equal 1, fs.children.length
        end

        def test_add_directory
          dir = @fs.directory('src')

          assert_equal 1, @fs.children.length
          assert_equal 'src', dir.name
          assert_instance_of Filesystem, dir
        end

        def test_add_nested_directories
          @fs.directory('src') do
            directory('lib') do
              directory 'helpers'
            end
          end

          src = @fs.children.first
          assert_equal 'src', src.name

          lib = src.children.first
          assert_equal 'lib', lib.name

          helpers = lib.children.first
          assert_equal 'helpers', helpers.name
        end

        def test_add_file
          file = @fs.file('test.rb', content: 'puts "test"')

          assert_equal 1, @fs.files.length
          assert_equal 'test.rb', file.name
          assert_equal 'puts "test"', file.content
        end

        def test_add_file_without_content
          file = @fs.file('empty.txt')

          assert_equal 'empty.txt', file.name
          assert_nil file.content
        end

        def test_add_file_with_attributes
          file = @fs.file('script.sh', content: '#!/bin/bash', executable: true, permissions: '0755')

          assert_equal true, file.attributes[:executable]
          assert_equal '0755', file.attributes[:permissions]
        end

        def test_add_multiple_files
          @fs.file('file1.txt', content: 'Content 1')
          @fs.file('file2.txt', content: 'Content 2')
          @fs.file('file3.txt', content: 'Content 3')

          assert_equal 3, @fs.files.length
        end

        def test_attr_set_and_get
          @fs.attr(:permissions, '0755')

          assert_equal '0755', @fs.attr(:permissions)
          assert_equal '0755', @fs.attributes[:permissions]
        end

        def test_attr_get_only
          @fs.attributes[:test] = 'value'

          assert_equal 'value', @fs.attr(:test)
        end

        def test_find_file_at_root
          @fs.file('test.txt', content: 'content')

          found = @fs.find_file('test.txt')

          refute_nil found
          assert_equal 'test.txt', found.name
        end

        def test_find_file_nested
          @fs.directory('src') do
            directory('lib') do
              file 'helper.rb', content: 'def help; end'
            end
          end

          found = @fs.find_file('src/lib/helper.rb')

          refute_nil found
          assert_equal 'helper.rb', found.name
          assert_equal 'def help; end', found.content
        end

        def test_find_file_not_found
          found = @fs.find_file('nonexistent.txt')

          assert_nil found
        end

        def test_find_file_empty_path
          found = @fs.find_file('')

          assert_nil found
        end

        def test_all_files
          @fs.file('root.txt')
          @fs.directory('src') do
            file 'main.rb'
            directory('lib') do
              file 'helper.rb'
            end
          end

          all = @fs.all_files

          assert_equal 3, all.length
          paths = all.map { |entry| entry[:path] }
          assert_includes paths, 'root/root.txt'
          assert_includes paths, 'root/src/main.rb'
          assert_includes paths, 'root/src/lib/helper.rb'
        end

        def test_all_files_with_prefix
          @fs.file('test.txt')

          all = @fs.all_files('prefix')

          assert_equal 'prefix/root/test.txt', all.first[:path]
        end

        def test_to_h
          @fs.file('test.txt', content: 'content')
          @fs.directory('src')
          @fs.attr(:key, 'value')

          hash = @fs.to_h

          assert_equal 'root', hash[:name]
          assert_equal 1, hash[:files].length
          assert_equal 1, hash[:directories].length
          assert_equal 'value', hash[:attributes][:key]
        end

        def test_tree_simple
          @fs.file('file1.txt')
          @fs.file('file2.txt')

          tree = @fs.tree

          assert_includes tree, 'root/'
          assert_includes tree, 'file1.txt'
          assert_includes tree, 'file2.txt'
        end

        def test_tree_nested
          @fs.directory('src') do
            file 'main.rb'
            directory('lib') do
              file 'helper.rb'
            end
          end

          tree = @fs.tree

          assert_includes tree, 'root/'
          assert_includes tree, 'src/'
          assert_includes tree, 'main.rb'
          assert_includes tree, 'lib/'
          assert_includes tree, 'helper.rb'
        end

        def test_tree_indentation
          @fs.directory('src') do
            directory('lib')
          end

          tree = @fs.tree

          lines = tree.split("\n")
          # Check that nested directories have proper indentation
          assert lines.any? { |line| line.start_with?('  src/') }
          assert lines.any? { |line| line.start_with?('    lib/') }
        end

        def test_complex_filesystem_structure
          fs = Filesystem.new('project') do
            file 'README.md', content: '# Project'
            file 'Gemfile', content: "source 'https://rubygems.org'"

            directory 'lib' do
              file 'main.rb', content: 'class Main; end'
              directory 'helpers' do
                file 'string_helper.rb'
                file 'number_helper.rb'
              end
            end

            directory 'test' do
              file 'test_main.rb'
            end

            attr :license, 'MIT'
          end

          assert_equal 'project', fs.name
          assert_equal 2, fs.files.length
          assert_equal 2, fs.children.length

          lib = fs.children.find { |c| c.name == 'lib' }
          assert_equal 1, lib.files.length
          assert_equal 1, lib.children.length

          helpers = lib.children.first
          assert_equal 2, helpers.files.length

          assert_equal 'MIT', fs.attr(:license)
        end

        # Edge case tests
        def test_empty_filesystem
          fs = Filesystem.new('empty')

          assert_empty fs.children
          assert_empty fs.files
          assert_empty fs.attributes
          assert_equal 'empty/', fs.tree.strip
        end

        def test_filesystem_with_only_directories
          fs = Filesystem.new('root') do
            directory 'dir1'
            directory 'dir2'
            directory 'dir3'
          end

          assert_equal 3, fs.children.length
          assert_empty fs.files
        end

        def test_filesystem_with_only_files
          fs = Filesystem.new('root') do
            file 'file1.txt'
            file 'file2.txt'
            file 'file3.txt'
          end

          assert_equal 3, fs.files.length
          assert_empty fs.children
        end

        def test_deeply_nested_directories
          fs = Filesystem.new('root') do
            directory 'level1' do
              directory 'level2' do
                directory 'level3' do
                  directory 'level4' do
                    directory 'level5' do
                      file 'deep.txt', content: 'very deep'
                    end
                  end
                end
              end
            end
          end

          found = fs.find_file('level1/level2/level3/level4/level5/deep.txt')
          refute_nil found
          assert_equal 'very deep', found.content
        end

        def test_find_file_with_leading_slash
          @fs.file('test.txt')
          found = @fs.find_file('/test.txt')

          # Leading slashes are rejected as empty parts, leaving 'test.txt'
          refute_nil found
          assert_equal 'test.txt', found.name
        end

        def test_find_file_with_multiple_slashes
          @fs.directory('src') do
            file 'main.rb'
          end

          found = @fs.find_file('src//main.rb')
          refute_nil found  # Multiple slashes are rejected as empty parts
        end

        def test_find_file_partial_path
          @fs.directory('src') do
            directory('lib') do
              file 'helper.rb'
            end
          end

          found = @fs.find_file('src')
          assert_nil found  # Can't find a directory as a file
        end

        def test_find_file_wrong_directory
          @fs.directory('src') do
            file 'main.rb'
          end
          @fs.directory('lib') do
            file 'helper.rb'
          end

          found = @fs.find_file('src/helper.rb')
          assert_nil found  # helper.rb is in lib, not src
        end

        def test_all_files_empty_filesystem
          all = @fs.all_files

          assert_empty all
        end

        def test_all_files_file_objects_included
          @fs.file('test.txt', content: 'content')

          all = @fs.all_files
          assert_equal 1, all.length
          assert_instance_of VirtualFile, all.first[:file]
          assert_equal 'test.txt', all.first[:file].name
        end

        def test_all_files_maintains_structure
          @fs.directory('a') do
            file 'a1.txt'
            directory('b') do
              file 'b1.txt'
            end
          end

          all = @fs.all_files
          paths = all.map { |e| e[:path] }

          assert_equal 2, paths.length
          assert_includes paths, 'root/a/a1.txt'
          assert_includes paths, 'root/a/b/b1.txt'
        end

        def test_attr_returns_nil_for_missing_key
          result = @fs.attr(:nonexistent)

          assert_nil result
        end

        def test_attr_with_nil_value_gets_attribute
          # When second argument is nil, attr acts as getter not setter
          @fs.attributes[:key] = 'value'

          result = @fs.attr(:key, nil)
          assert_equal 'value', result
        end

        def test_attr_with_various_types
          @fs.attr(:string, 'value')
          @fs.attr(:number, 42)
          @fs.attr(:boolean, true)
          @fs.attr(:array, [1, 2, 3])
          @fs.attr(:hash, { nested: 'data' })

          assert_equal 'value', @fs.attr(:string)
          assert_equal 42, @fs.attr(:number)
          assert_equal true, @fs.attr(:boolean)
          assert_equal [1, 2, 3], @fs.attr(:array)
          assert_equal({ nested: 'data' }, @fs.attr(:hash))
        end

        def test_directory_returns_created_directory
          dir = @fs.directory('test')

          assert_instance_of Filesystem, dir
          assert_equal 'test', dir.name
          assert_same dir, @fs.children.first
        end

        def test_file_returns_created_file
          file = @fs.file('test.txt', content: 'content')

          assert_instance_of VirtualFile, file
          assert_equal 'test.txt', file.name
          assert_same file, @fs.files.first
        end

        def test_multiple_directories_with_same_name
          @fs.directory('duplicate')
          @fs.directory('duplicate')

          assert_equal 2, @fs.children.length
          assert_equal 'duplicate', @fs.children[0].name
          assert_equal 'duplicate', @fs.children[1].name
        end

        def test_multiple_files_with_same_name
          @fs.file('duplicate.txt', content: 'first')
          @fs.file('duplicate.txt', content: 'second')

          assert_equal 2, @fs.files.length
          assert_equal 'first', @fs.files[0].content
          assert_equal 'second', @fs.files[1].content
        end

        def test_to_h_empty_filesystem
          hash = @fs.to_h

          assert_equal 'root', hash[:name]
          assert_empty hash[:files]
          assert_empty hash[:directories]
          assert_empty hash[:attributes]
        end

        def test_to_h_nested_structure
          @fs.directory('src') do
            file 'main.rb', content: 'code'
          end

          hash = @fs.to_h

          assert_equal 1, hash[:directories].length
          src_hash = hash[:directories].first
          assert_equal 'src', src_hash[:name]
          assert_equal 1, src_hash[:files].length
          assert_equal 'main.rb', src_hash[:files].first[:name]
        end

        def test_tree_empty_directory
          @fs.directory('empty')

          tree = @fs.tree

          assert_includes tree, 'empty/'
          lines = tree.split("\n").select { |l| l.include?('empty/') }
          assert_equal 1, lines.length
        end

        def test_tree_multiple_levels
          @fs.directory('level1') do
            directory('level2') do
              directory 'level3'
            end
          end

          tree = @fs.tree
          lines = tree.split("\n")

          assert lines.any? { |l| l.match(/^\s*root\//) }
          assert lines.any? { |l| l.match(/^\s{2}level1\//) }
          assert lines.any? { |l| l.match(/^\s{4}level2\//) }
          assert lines.any? { |l| l.match(/^\s{6}level3\//) }
        end

        def test_tree_with_custom_indent
          @fs.directory('subdir')

          tree = @fs.tree(4)  # Start with 4 spaces

          lines = tree.split("\n")
          assert lines.any? { |l| l.start_with?('    root/') }
          assert lines.any? { |l| l.start_with?('      subdir/') }
        end

        def test_tree_files_before_directories
          @fs.directory('zzz_dir')
          @fs.file('aaa_file.txt')

          tree = @fs.tree
          lines = tree.split("\n").reject(&:empty?)

          # Files should appear before directories in tree output
          file_index = lines.index { |l| l.include?('aaa_file.txt') }
          dir_index = lines.index { |l| l.include?('zzz_dir/') }

          assert file_index < dir_index
        end
      end

      class TestVirtualFile < Minitest::Test
        def test_initialize
          file = VirtualFile.new('test.txt', 'content', { key: 'value' })

          assert_equal 'test.txt', file.name
          assert_equal 'content', file.content
          assert_equal 'value', file.attributes[:key]
        end

        def test_initialize_minimal
          file = VirtualFile.new('test.txt')

          assert_equal 'test.txt', file.name
          assert_nil file.content
          assert_empty file.attributes
        end

        def test_content_immutable
          file = VirtualFile.new('test.txt')
          new_file = file.with_content('new content')

          assert_equal 'new content', new_file.content
          assert_nil file.content  # Original unchanged
        end

        def test_to_h
          file = VirtualFile.new('test.txt', 'content', { executable: true })

          hash = file.to_h

          assert_equal 'test.txt', hash[:name]
          assert_equal 'content', hash[:content]
          assert_equal true, hash[:attributes][:executable]
        end

        # Validation and error tests
        def test_nil_filename_raises_error
          error = assert_raises(InvalidNameError) do
            VirtualFile.new(nil)
          end
          assert_equal 'Filename cannot be nil', error.message
        end

        def test_empty_filename_raises_error
          error = assert_raises(InvalidNameError) do
            VirtualFile.new('')
          end
          assert_equal 'Filename cannot be empty', error.message
        end

        def test_non_string_filename_raises_error
          error = assert_raises(InvalidNameError) do
            VirtualFile.new(123)
          end
          assert_match(/Filename must be a String/, error.message)
        end

        def test_filename_with_slash_raises_error
          error = assert_raises(InvalidPathError) do
            VirtualFile.new('path/to/file.txt')
          end
          assert_equal 'Filename cannot contain path separators', error.message
        end

        def test_filename_with_null_byte_raises_error
          error = assert_raises(InvalidPathError) do
            VirtualFile.new("file\x00.txt")
          end
          assert_equal 'Filename cannot contain null bytes', error.message
        end

        def test_non_hash_attributes_raises_error
          error = assert_raises(ValidationError) do
            VirtualFile.new('test.txt', 'content', 'not a hash')
          end
          assert_match(/Attributes must be a Hash/, error.message)
        end

        def test_file_is_frozen
          file = VirtualFile.new('test.txt', 'content')
          assert file.frozen?
        end

        def test_attributes_are_frozen
          file = VirtualFile.new('test.txt', 'content', { key: 'value' })
          assert file.attributes.frozen?
        end

        def test_with_content_returns_new_instance
          file = VirtualFile.new('test.txt', 'original')
          new_file = file.with_content('updated')

          refute_same file, new_file
          assert_equal 'original', file.content
          assert_equal 'updated', new_file.content
          assert_equal file.name, new_file.name
          assert_equal file.attributes, new_file.attributes
        end

        def test_with_content_preserves_attributes
          file = VirtualFile.new('test.txt', 'content', { executable: true, mode: '0755' })
          new_file = file.with_content('new content')

          assert_equal true, new_file.attributes[:executable]
          assert_equal '0755', new_file.attributes[:mode]
        end

        def test_with_attributes_returns_new_instance
          file = VirtualFile.new('test.txt', 'content', { key: 'value' })
          new_file = file.with_attributes({ key2: 'value2' })

          refute_same file, new_file
          assert_equal 'value', file.attributes[:key]
          refute file.attributes.key?(:key2)
          assert_equal 'value', new_file.attributes[:key]
          assert_equal 'value2', new_file.attributes[:key2]
        end

        def test_with_attributes_merges_correctly
          file = VirtualFile.new('test.txt', 'content', { a: 1, b: 2 })
          new_file = file.with_attributes({ b: 3, c: 4 })

          assert_equal 1, new_file.attributes[:a]
          assert_equal 3, new_file.attributes[:b]  # Updated
          assert_equal 4, new_file.attributes[:c]  # New
        end

        def test_with_attributes_preserves_content
          file = VirtualFile.new('test.txt', 'original content')
          new_file = file.with_attributes({ key: 'value' })

          assert_equal 'original content', new_file.content
        end

        def test_equality_same_files
          file1 = VirtualFile.new('test.txt', 'content', { key: 'value' })
          file2 = VirtualFile.new('test.txt', 'content', { key: 'value' })

          assert_equal file1, file2
          assert file1.eql?(file2)
        end

        def test_equality_different_names
          file1 = VirtualFile.new('test1.txt', 'content')
          file2 = VirtualFile.new('test2.txt', 'content')

          refute_equal file1, file2
        end

        def test_equality_different_content
          file1 = VirtualFile.new('test.txt', 'content1')
          file2 = VirtualFile.new('test.txt', 'content2')

          refute_equal file1, file2
        end

        def test_equality_different_attributes
          file1 = VirtualFile.new('test.txt', 'content', { key: 'value1' })
          file2 = VirtualFile.new('test.txt', 'content', { key: 'value2' })

          refute_equal file1, file2
        end

        def test_equality_nil_vs_empty_content
          file1 = VirtualFile.new('test.txt', nil)
          file2 = VirtualFile.new('test.txt', '')

          refute_equal file1, file2
        end

        def test_hash_same_for_equal_files
          file1 = VirtualFile.new('test.txt', 'content', { key: 'value' })
          file2 = VirtualFile.new('test.txt', 'content', { key: 'value' })

          assert_equal file1.hash, file2.hash
        end

        def test_hash_different_for_unequal_files
          file1 = VirtualFile.new('test1.txt', 'content')
          file2 = VirtualFile.new('test2.txt', 'content')

          refute_equal file1.hash, file2.hash
        end

        def test_to_h_includes_all_fields
          file = VirtualFile.new('test.txt', 'content', { executable: true })
          hash = file.to_h

          assert_instance_of Hash, hash
          assert_equal 'test.txt', hash[:name]
          assert_equal 'content', hash[:content]
          assert_instance_of Hash, hash[:attributes]
          assert_equal true, hash[:attributes][:executable]
        end

        def test_to_h_with_nil_content
          file = VirtualFile.new('test.txt')
          hash = file.to_h

          assert_nil hash[:content]
        end

        def test_to_h_with_empty_attributes
          file = VirtualFile.new('test.txt', 'content')
          hash = file.to_h

          assert_empty hash[:attributes]
        end

        def test_various_valid_filenames
          valid_names = [
            'simple.txt',
            'file-with-dashes.rb',
            'file_with_underscores.py',
            'file.multiple.dots.txt',
            'FILE_UPPERCASE.TXT',
            '123numeric.txt',
            'unicode_文件.txt',
            '.hidden',
            '..double-dot',
            'no-extension'
          ]

          valid_names.each do |name|
            file = VirtualFile.new(name)
            assert_equal name, file.name
          end
        end

        def test_content_can_be_multiline
          content = "Line 1\nLine 2\nLine 3"
          file = VirtualFile.new('test.txt', content)

          assert_equal content, file.content
        end

        def test_content_can_be_binary
          binary_content = "\x00\x01\x02\xFF"
          file = VirtualFile.new('test.bin', binary_content)

          assert_equal binary_content, file.content
        end

        def test_attributes_can_be_complex
          attrs = {
            string: 'value',
            number: 42,
            boolean: true,
            array: [1, 2, 3],
            hash: { nested: 'data' },
            nil_value: nil
          }
          file = VirtualFile.new('test.txt', nil, attrs)

          assert_equal attrs, file.attributes
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
