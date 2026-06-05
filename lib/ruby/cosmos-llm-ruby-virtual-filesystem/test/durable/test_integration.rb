# frozen_string_literal: true

require 'test_helper'

module Cosmos
  module Llm
    module VirtualFilesystem
      # Integration and stress tests for VirtualFilesystem
      class TestIntegration < Minitest::Test
        # Integration tests combining multiple features
        def test_building_and_querying_complex_filesystem
          fs = Filesystem.new('/') do
            file '.gitignore', content: '*.log'
            file 'README.md', content: '# Project'

            directory 'src' do
              file 'index.js', content: 'console.log("hello")', type: 'javascript'
              directory 'components' do
                file 'Header.jsx', content: 'export const Header', type: 'jsx'
                file 'Footer.jsx', content: 'export const Footer', type: 'jsx'
              end
              directory 'utils' do
                file 'helpers.js', content: 'export function help()'
              end
            end

            directory 'public' do
              file 'index.html', content: '<!DOCTYPE html>'
              directory 'assets' do
                file 'logo.png', content: 'binary data', binary: true
                file 'style.css', content: 'body { margin: 0; }'
              end
            end

            directory 'test' do
              file 'setup.js'
              file 'index.test.js'
            end

            attr :name, 'my-project'
            attr :version, '1.0.0'
            attr :language, 'javascript'
          end

          # Query tests
          assert_equal 2, fs.files.length
          assert_equal 3, fs.children.length

          # Find specific files
          header = fs.find_file('src/components/Header.jsx')
          refute_nil header
          assert_equal 'export const Header', header.content
          assert_equal 'jsx', header.attributes[:type]

          logo = fs.find_file('public/assets/logo.png')
          refute_nil logo
          assert_equal true, logo.attributes[:binary]

          # List all files
          all = fs.all_files
          assert_equal 11, all.length

          # Check attributes
          assert_equal 'my-project', fs.attr(:name)
          assert_equal '1.0.0', fs.attr(:version)

          # Tree visualization
          tree = fs.tree
          assert_includes tree, '//'
          assert_includes tree, 'src/'
          assert_includes tree, 'components/'
          assert_includes tree, 'Header.jsx'
        end

        def test_modifying_filesystem_structure
          fs = Filesystem.new('root')

          # Start empty
          assert_empty fs.files
          assert_empty fs.children

          # Add initial structure
          fs.file('initial.txt', content: 'first')
          src = fs.directory('src')

          assert_equal 1, fs.files.length
          assert_equal 1, fs.children.length

          # Add to subdirectory
          src.file('main.rb', content: 'puts "hi"')
          src.directory('lib').file('helper.rb')

          # Verify structure
          assert_equal 1, src.files.length
          assert_equal 1, src.children.length

          found = fs.find_file('src/main.rb')
          refute_nil found
          assert_equal 'puts "hi"', found.content

          found_helper = fs.find_file('src/lib/helper.rb')
          refute_nil found_helper

          # Add more at root level
          fs.file('another.txt')
          assert_equal 2, fs.files.length

          # Verify all files
          all = fs.all_files
          assert_equal 4, all.length
        end

        def test_cloning_with_modifications
          original = Filesystem.new('original') do
            file 'config.yml', content: 'debug: true'
            directory 'src' do
              file 'main.rb', content: 'original code'
            end
          end

          # Create a similar but modified structure
          modified = Filesystem.new('modified') do
            file 'config.yml', content: 'debug: false'  # Changed content
            directory 'src' do
              file 'main.rb', content: 'original code'
            end
          end

          # Structures should not be equal due to name difference
          refute_equal original, modified

          # Even if we make names the same, content differs
          same_name = Filesystem.new('original') do
            file 'config.yml', content: 'debug: false'
            directory 'src' do
              file 'main.rb', content: 'original code'
            end
          end

          refute_equal original, same_name

          # Create exact clone
          clone = Filesystem.new('original') do
            file 'config.yml', content: 'debug: true'
            directory 'src' do
              file 'main.rb', content: 'original code'
            end
          end

          assert_equal original, clone
        end

        def test_immutable_virtualfile_integration
          fs = Filesystem.new('root')
          original_file = fs.file('data.txt', content: 'original', version: 1)

          # Create modified versions
          updated_content = original_file.with_content('updated')
          updated_attrs = original_file.with_attributes({ version: 2 })

          # Original file unchanged
          assert_equal 'original', original_file.content
          assert_equal 1, original_file.attributes[:version]

          # New versions have changes
          assert_equal 'updated', updated_content.content
          assert_equal 1, updated_content.attributes[:version]

          assert_equal 'original', updated_attrs.content
          assert_equal 2, updated_attrs.attributes[:version]

          # All are different objects
          refute_same original_file, updated_content
          refute_same original_file, updated_attrs
          refute_same updated_content, updated_attrs

          # But original file in filesystem is unchanged
          assert_equal 'original', fs.files.first.content
        end

        def test_to_h_round_trip_compatibility
          original = Filesystem.new('project') do
            file 'README.md', content: '# Title', lang: 'markdown'
            directory 'src' do
              file 'main.rb', content: 'code'
              attr :purpose, 'source code'
            end
            attr :license, 'MIT'
          end

          hash = original.to_h

          # Verify hash structure is complete and correct
          assert_equal 'project', hash[:name]
          assert_equal 1, hash[:files].length
          assert_equal 'README.md', hash[:files][0][:name]
          assert_equal '# Title', hash[:files][0][:content]
          assert_equal 'markdown', hash[:files][0][:attributes][:lang]

          assert_equal 1, hash[:directories].length
          assert_equal 'src', hash[:directories][0][:name]
          assert_equal 'source code', hash[:directories][0][:attributes][:purpose]

          assert_equal 'MIT', hash[:attributes][:license]

          # Could rebuild from hash if needed
          assert_instance_of Hash, hash
          assert hash[:files].all? { |f| f.is_a?(Hash) }
          assert hash[:directories].all? { |d| d.is_a?(Hash) }
        end

        def test_stress_many_files
          fs = Filesystem.new('large')

          # Add 1000 files
          1000.times do |i|
            fs.file("file_#{i}.txt", content: "Content #{i}", index: i)
          end

          assert_equal 1000, fs.files.length

          # All files should be retrievable
          all = fs.all_files
          assert_equal 1000, all.length

          # Spot check some files
          assert_equal 'file_0.txt', fs.files[0].name
          assert_equal 'file_999.txt', fs.files[999].name
          assert_equal 'Content 500', fs.files[500].content
          assert_equal 250, fs.files[250].attributes[:index]

          # Tree should include all files
          tree = fs.tree
          assert_includes tree, 'file_0.txt'
          assert_includes tree, 'file_500.txt'
          assert_includes tree, 'file_999.txt'

          # to_h should work
          hash = fs.to_h
          assert_equal 1000, hash[:files].length
        end

        def test_stress_many_directories
          fs = Filesystem.new('wide')

          # Add 500 directories
          500.times do |i|
            fs.directory("dir_#{i}")
          end

          assert_equal 500, fs.children.length

          # Tree should include all directories
          tree = fs.tree
          assert_includes tree, 'dir_0/'
          assert_includes tree, 'dir_499/'

          # to_h should work
          hash = fs.to_h
          assert_equal 500, hash[:directories].length
        end

        def test_stress_deep_nesting
          # Create 100-level deep nesting
          fs = Filesystem.new('root')
          current = fs

          100.times do |i|
            current = current.directory("level_#{i}")
          end

          # Add file at the deepest level
          current.file('deep.txt', content: 'very deep')

          # Build path
          path_parts = (0...100).map { |i| "level_#{i}" }
          path = path_parts.join('/') + '/deep.txt'

          # Should be able to find it
          found = fs.find_file(path)
          refute_nil found
          assert_equal 'very deep', found.content

          # Should be in all_files
          all = fs.all_files
          assert_equal 1, all.length
          assert all.first[:path].include?('deep.txt')
        end

        def test_unicode_content_and_names
          fs = Filesystem.new('root') do
            file '日本語.txt', content: 'こんにちは世界'
            file 'emoji_🎉.txt', content: '🎊🎈🎁'
            directory 'français' do
              file 'données.txt', content: 'Héllo Wörld'
            end
            directory '中文' do
              file '文件.txt', content: '你好'
            end
          end

          # Find files with unicode names
          japanese = fs.find_file('日本語.txt')
          refute_nil japanese
          assert_equal 'こんにちは世界', japanese.content

          french = fs.find_file('français/données.txt')
          refute_nil french
          assert_equal 'Héllo Wörld', french.content

          chinese = fs.find_file('中文/文件.txt')
          refute_nil chinese
          assert_equal '你好', chinese.content

          # Tree should handle unicode
          tree = fs.tree
          assert_includes tree, '日本語.txt'
          assert_includes tree, 'emoji_🎉.txt'
          assert_includes tree, 'français/'
          assert_includes tree, '中文/'
        end

        def test_empty_and_whitespace_content
          fs = Filesystem.new('root') do
            file 'empty.txt', content: ''
            file 'spaces.txt', content: '   '
            file 'newlines.txt', content: "\n\n\n"
            file 'tabs.txt', content: "\t\t\t"
            file 'mixed.txt', content: " \n\t \n "
          end

          assert_equal '', fs.find_file('empty.txt').content
          assert_equal '   ', fs.find_file('spaces.txt').content
          assert_equal "\n\n\n", fs.find_file('newlines.txt').content
          assert_equal "\t\t\t", fs.find_file('tabs.txt').content
          assert_equal " \n\t \n ", fs.find_file('mixed.txt').content

          # All should be distinct
          contents = fs.files.map(&:content).uniq
          assert_equal 5, contents.length
        end

        def test_attribute_keys_and_values_variety
          fs = Filesystem.new('root')

          # String keys
          fs.attr(:string_key, 'value')
          fs.attr('string_key_as_string', 'value2')

          # Various value types
          fs.attr(:nil_value, nil)
          fs.attr(:false_value, false)
          fs.attr(:true_value, true)
          fs.attr(:zero, 0)
          fs.attr(:negative, -1)
          fs.attr(:float, 3.14)
          fs.attr(:array, [1, 2, 3])
          fs.attr(:hash, { nested: { deep: 'value' } })
          fs.attr(:symbol, :symbol_value)

          assert_equal 'value', fs.attr(:string_key)
          assert_equal 'value2', fs.attr('string_key_as_string')
          assert_nil fs.attr(:nil_value)
          assert_equal false, fs.attr(:false_value)
          assert_equal true, fs.attr(:true_value)
          assert_equal 0, fs.attr(:zero)
          assert_equal(-1, fs.attr(:negative))
          assert_equal 3.14, fs.attr(:float)
          assert_equal [1, 2, 3], fs.attr(:array)
          assert_equal({ nested: { deep: 'value' } }, fs.attr(:hash))
          assert_equal :symbol_value, fs.attr(:symbol)
        end

        def test_combining_filesystem_data
          # Simulate merging metadata from multiple filesystems
          fs1 = Filesystem.new('project')
          fs1.attr(:created_by, 'user1')
          fs1.attr(:version, '1.0')

          fs2 = Filesystem.new('project')
          fs2.attr(:modified_by, 'user2')
          fs2.attr(:version, '1.1')

          # Combine attributes manually
          combined = Filesystem.new('project')
          fs1.attributes.each { |k, v| combined.attr(k, v) }
          fs2.attributes.each { |k, v| combined.attr(k, v) }

          assert_equal 'user1', combined.attr(:created_by)
          assert_equal 'user2', combined.attr(:modified_by)
          assert_equal '1.1', combined.attr(:version)  # fs2 overwrites
        end

        def test_path_edge_cases
          fs = Filesystem.new('root') do
            directory 'a' do
              directory 'b' do
                file 'c.txt'
              end
            end
          end

          # Normal path
          assert fs.find_file('a/b/c.txt')

          # Trailing slash creates empty part which is rejected, leaving 'a/b/c.txt'
          assert fs.find_file('a/b/c.txt/')

          # Extra slashes (empty parts are rejected)
          assert fs.find_file('a//b//c.txt')

          # Just directory name (can't find directory as file)
          assert_nil fs.find_file('a')
          assert_nil fs.find_file('a/b')

          # Nonexistent path
          assert_nil fs.find_file('a/b/d.txt')
          assert_nil fs.find_file('x/y/z.txt')
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
