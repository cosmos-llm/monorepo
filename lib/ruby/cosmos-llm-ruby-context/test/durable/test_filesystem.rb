# frozen_string_literal: true

require 'test_helper'

module Cosmos
  module Llm
    module Context
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
          assert(lines.any? { |line| line.start_with?('  src/') })
          assert(lines.any? { |line| line.start_with?('    lib/') })
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

            attr :license, 'MIT' # rubocop:disable Naming/MethodName
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

        def test_empty_filesystem_operations
          fs = Filesystem.new('empty')

          assert_nil fs.find_file('nonexistent')
          assert_empty fs.all_files
          assert_equal 'empty/', fs.tree
        end

        def test_filesystem_with_special_characters
          fs = Filesystem.new('test-dir')
          fs.file('file with spaces.txt', content: 'content')
          fs.file('file-with-dashes.txt')
          fs.file('file_with_underscores.txt')

          found = fs.find_file('file with spaces.txt')
          assert_equal 'content', found.content

          all = fs.all_files
          assert_equal 3, all.length
        end

        def test_nested_empty_directories
          fs = Filesystem.new('root')
          fs.directory('empty1') do
            directory('empty2') do
              directory('empty3')
            end
          end

          tree = fs.tree
          assert_includes tree, 'empty1/'
          assert_includes tree, 'empty2/'
          assert_includes tree, 'empty3/'

          assert_empty fs.all_files
        end

        def test_file_with_nil_content
          file = @fs.file('nil_content.txt', content: nil)
          assert_nil file.content
          assert_equal 'nil_content.txt', file.name
        end

        def test_attr_operations
          # Test setting and getting
          @fs.attr(:key1, 'value1')
          assert_equal 'value1', @fs.attr(:key1)

          # Test getting non-existent key
          assert_nil @fs.attr(:nonexistent)

          # Test overwriting
          @fs.attr(:key1, 'new_value')
          assert_equal 'new_value', @fs.attr(:key1)
        end

        def test_find_file_edge_cases
          @fs.file('file.txt')

          # Empty string
          assert_nil @fs.find_file('')

          # Nil
          assert_nil @fs.find_file(nil)

          # Path with only slashes
          assert_nil @fs.find_file('///')

          # Non-existent nested path
          assert_nil @fs.find_file('nonexistent/file.txt')
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
          assert_nil file.content # Original unchanged
        end

        def test_to_h
          file = VirtualFile.new('test.txt', 'content', { executable: true })

          hash = file.to_h

          assert_equal 'test.txt', hash[:name]
          assert_equal 'content', hash[:content]
          assert_equal true, hash[:attributes][:executable]
        end
      end
    end
  end

  class TestFilesystemAliases < Minitest::Test
    def test_filesystem_alias
      assert_equal Cosmos::Llm::VirtualFilesystem::Filesystem, Cosmos::Llm::Context::Filesystem
    end

    def test_virtual_file_alias
      assert_equal Cosmos::Llm::VirtualFilesystem::VirtualFile, Cosmos::Llm::Context::VirtualFile
    end

    def test_filesystem_alias_functionality
      fs = Cosmos::Llm::Context::Filesystem.new('test')
      assert_equal 'test', fs.name
      assert_instance_of Cosmos::Llm::VirtualFilesystem::Filesystem, fs
    end

    def test_virtual_file_alias_functionality
      file = Cosmos::Llm::Context::VirtualFile.new('test.txt', 'content')
      assert_equal 'test.txt', file.name
      assert_equal 'content', file.content
      assert_instance_of Cosmos::Llm::VirtualFilesystem::VirtualFile, file
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
