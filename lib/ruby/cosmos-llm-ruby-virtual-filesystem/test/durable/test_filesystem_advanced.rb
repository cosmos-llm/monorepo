# frozen_string_literal: true

require 'test_helper'

module Cosmos
  module Llm
    module VirtualFilesystem
      # Advanced tests for Filesystem including equality, hashing, and complex scenarios
      class TestFilesystemAdvanced < Minitest::Test
        def setup
          @fs = Filesystem.new('root')
        end

        # Equality tests
        def test_equality_empty_filesystems
          fs1 = Filesystem.new('root')
          fs2 = Filesystem.new('root')

          assert_equal fs1, fs2
          assert fs1.eql?(fs2)
        end

        def test_equality_different_names
          fs1 = Filesystem.new('root1')
          fs2 = Filesystem.new('root2')

          refute_equal fs1, fs2
        end

        def test_equality_same_files
          fs1 = Filesystem.new('root') do
            file 'test.txt', content: 'content'
          end
          fs2 = Filesystem.new('root') do
            file 'test.txt', content: 'content'
          end

          assert_equal fs1, fs2
        end

        def test_equality_different_files
          fs1 = Filesystem.new('root') do
            file 'test1.txt'
          end
          fs2 = Filesystem.new('root') do
            file 'test2.txt'
          end

          refute_equal fs1, fs2
        end

        def test_equality_same_directories
          fs1 = Filesystem.new('root') do
            directory 'src'
          end
          fs2 = Filesystem.new('root') do
            directory 'src'
          end

          assert_equal fs1, fs2
        end

        def test_equality_different_directories
          fs1 = Filesystem.new('root') do
            directory 'src'
          end
          fs2 = Filesystem.new('root') do
            directory 'lib'
          end

          refute_equal fs1, fs2
        end

        def test_equality_same_attributes
          fs1 = Filesystem.new('root') do
            attr :license, 'MIT'
          end
          fs2 = Filesystem.new('root') do
            attr :license, 'MIT'
          end

          assert_equal fs1, fs2
        end

        def test_equality_different_attributes
          fs1 = Filesystem.new('root') do
            attr :license, 'MIT'
          end
          fs2 = Filesystem.new('root') do
            attr :license, 'GPL'
          end

          refute_equal fs1, fs2
        end

        def test_equality_complex_identical_structure
          fs1 = Filesystem.new('project') do
            file 'README.md', content: '# Project'
            directory 'src' do
              file 'main.rb', content: 'puts "hi"'
              directory 'lib' do
                file 'helper.rb'
              end
            end
            attr :license, 'MIT'
          end

          fs2 = Filesystem.new('project') do
            file 'README.md', content: '# Project'
            directory 'src' do
              file 'main.rb', content: 'puts "hi"'
              directory 'lib' do
                file 'helper.rb'
              end
            end
            attr :license, 'MIT'
          end

          assert_equal fs1, fs2
        end

        def test_equality_not_filesystem
          fs = Filesystem.new('root')

          refute_equal fs, 'not a filesystem'
          refute_equal fs, nil
          refute_equal fs, []
        end

        # Hashing tests
        def test_hash_same_for_equal_filesystems
          fs1 = Filesystem.new('root') do
            file 'test.txt'
          end
          fs2 = Filesystem.new('root') do
            file 'test.txt'
          end

          assert_equal fs1.hash, fs2.hash
        end

        def test_hash_different_for_unequal_filesystems
          fs1 = Filesystem.new('root1')
          fs2 = Filesystem.new('root2')

          refute_equal fs1.hash, fs2.hash
        end

        def test_hash_can_be_used_in_hash_map
          fs1 = Filesystem.new('root') do
            file 'test.txt'
          end
          fs2 = Filesystem.new('root') do
            file 'test.txt'
          end

          hash_map = { fs1 => 'value1' }
          hash_map[fs2] = 'value2'

          # Since fs1 == fs2, they should map to the same hash key
          assert_equal 1, hash_map.size
          assert_equal 'value2', hash_map[fs1]
        end

        # Complex nested scenarios
        def test_large_flat_directory
          fs = Filesystem.new('root')
          100.times do |i|
            fs.file("file_#{i}.txt", content: "Content #{i}")
          end

          assert_equal 100, fs.files.length
          all_files = fs.all_files
          assert_equal 100, all_files.length
        end

        def test_wide_directory_structure
          fs = Filesystem.new('root')
          50.times do |i|
            fs.directory("dir_#{i}")
          end

          assert_equal 50, fs.children.length
          tree = fs.tree
          assert_includes tree, 'dir_0/'
          assert_includes tree, 'dir_49/'
        end

        def test_mixed_content_at_all_levels
          fs = Filesystem.new('root') do
            file 'root_file.txt'
            directory 'level1' do
              file 'level1_file.txt'
              directory 'level2' do
                file 'level2_file.txt'
                directory 'level3' do
                  file 'level3_file.txt'
                end
              end
            end
          end

          all = fs.all_files
          assert_equal 4, all.length

          paths = all.map { |e| e[:path] }
          assert_includes paths, 'root/root_file.txt'
          assert_includes paths, 'root/level1/level1_file.txt'
          assert_includes paths, 'root/level1/level2/level2_file.txt'
          assert_includes paths, 'root/level1/level2/level3/level3_file.txt'
        end

        def test_realistic_ruby_project_structure
          fs = Filesystem.new('my_gem') do
            file 'README.md', content: '# My Gem'
            file 'Gemfile', content: 'source "https://rubygems.org"'
            file 'Rakefile'
            file '.gitignore'
            file 'my_gem.gemspec'

            directory 'lib' do
              file 'my_gem.rb'
              directory 'my_gem' do
                file 'version.rb', content: 'VERSION = "0.1.0"'
                file 'configuration.rb'
                file 'client.rb'
              end
            end

            directory 'test' do
              file 'test_helper.rb'
              directory 'my_gem' do
                file 'test_client.rb'
                file 'test_configuration.rb'
              end
            end

            directory 'bin' do
              file 'console', executable: true
              file 'setup', executable: true
            end

            attr :license, 'MIT'
            attr :version, '0.1.0'
          end

          # Validate structure
          assert_equal 'my_gem', fs.name
          assert_equal 5, fs.files.length
          assert_equal 3, fs.children.length

          # Check lib directory
          lib = fs.children.find { |c| c.name == 'lib' }
          assert_equal 1, lib.files.length
          assert_equal 1, lib.children.length

          # Check nested my_gem directory
          my_gem_dir = lib.children.first
          assert_equal 'my_gem', my_gem_dir.name
          assert_equal 3, my_gem_dir.files.length

          # Check file finding
          version_file = fs.find_file('lib/my_gem/version.rb')
          refute_nil version_file
          assert_equal 'VERSION = "0.1.0"', version_file.content

          # Check attributes
          assert_equal 'MIT', fs.attr(:license)
          assert_equal '0.1.0', fs.attr(:version)

          # Check all files count
          # Root: 5, lib: 1, lib/my_gem: 3, test: 1, test/my_gem: 2, bin: 2 = 14 total
          all_files = fs.all_files
          assert_equal 14, all_files.length
        end

        def test_filesystem_with_special_characters_in_names
          fs = Filesystem.new('root') do
            directory 'dir-with-dashes' do
              file 'file_with_underscores.txt'
            end
            directory 'dir.with.dots' do
              file 'file.multiple.dots.txt'
            end
            file '123_starts_with_number.txt'
          end

          found1 = fs.find_file('dir-with-dashes/file_with_underscores.txt')
          found2 = fs.find_file('dir.with.dots/file.multiple.dots.txt')

          refute_nil found1
          refute_nil found2
          assert fs.files.any? { |f| f.name == '123_starts_with_number.txt' }
        end

        def test_filesystem_mutation_after_creation
          fs = Filesystem.new('root') do
            file 'initial.txt'
          end

          # Add more files after creation
          fs.file('added_later.txt', content: 'new content')
          fs.directory('new_dir') do
            file 'nested.txt'
          end

          assert_equal 2, fs.files.length
          assert_equal 1, fs.children.length
          found = fs.find_file('new_dir/nested.txt')
          refute_nil found
        end

        def test_deeply_nested_with_many_siblings
          fs = Filesystem.new('root') do
            directory 'branch1' do
              directory 'subbranch1' do
                file 'file1.txt'
                file 'file2.txt'
              end
              directory 'subbranch2' do
                file 'file3.txt'
              end
            end
            directory 'branch2' do
              directory 'subbranch3' do
                file 'file4.txt'
              end
            end
          end

          all = fs.all_files
          assert_equal 4, all.length

          paths = all.map { |e| e[:path] }
          assert_includes paths, 'root/branch1/subbranch1/file1.txt'
          assert_includes paths, 'root/branch1/subbranch1/file2.txt'
          assert_includes paths, 'root/branch1/subbranch2/file3.txt'
          assert_includes paths, 'root/branch2/subbranch3/file4.txt'
        end

        def test_to_h_preserves_complete_structure
          fs = Filesystem.new('root') do
            file 'f1.txt', content: 'c1', metadata: 'value'
            directory 'dir' do
              file 'f2.txt', content: 'c2'
            end
            attr :key, 'value'
          end

          hash = fs.to_h

          # Check root level
          assert_equal 'root', hash[:name]
          assert_equal 1, hash[:files].length
          assert_equal 1, hash[:directories].length
          assert_equal 'value', hash[:attributes][:key]

          # Check file details
          file_hash = hash[:files].first
          assert_equal 'f1.txt', file_hash[:name]
          assert_equal 'c1', file_hash[:content]
          assert_equal 'value', file_hash[:attributes][:metadata]

          # Check directory details
          dir_hash = hash[:directories].first
          assert_equal 'dir', dir_hash[:name]
          assert_equal 1, dir_hash[:files].length
          assert_equal 'f2.txt', dir_hash[:files].first[:name]
        end

        def test_tree_output_format_comprehensive
          fs = Filesystem.new('project') do
            file 'README.md'
            file 'Gemfile'
            directory 'lib' do
              file 'main.rb'
              directory 'helpers' do
                file 'string.rb'
                file 'number.rb'
              end
            end
            directory 'test' do
              file 'test_main.rb'
            end
          end

          tree = fs.tree

          # Check all entries are present
          assert_includes tree, 'project/'
          assert_includes tree, 'README.md'
          assert_includes tree, 'Gemfile'
          assert_includes tree, 'lib/'
          assert_includes tree, 'main.rb'
          assert_includes tree, 'helpers/'
          assert_includes tree, 'string.rb'
          assert_includes tree, 'number.rb'
          assert_includes tree, 'test/'
          assert_includes tree, 'test_main.rb'

          # Verify indentation structure
          lines = tree.split("\n")
          project_line = lines.find { |l| l.include?('project/') }
          lib_line = lines.find { |l| l.include?('lib/') && !l.include?('project') }
          helpers_line = lines.find { |l| l.include?('helpers/') }

          assert_match(/^project\//, project_line)
          assert_match(/^  lib\//, lib_line)
          assert_match(/^    helpers\//, helpers_line)
        end

        def test_find_file_case_sensitive
          @fs.file('Test.txt')
          @fs.file('test.txt')

          found_upper = @fs.find_file('Test.txt')
          found_lower = @fs.find_file('test.txt')

          refute_nil found_upper
          refute_nil found_lower
          refute_same found_upper, found_lower
        end

        def test_all_files_order_preservation
          fs = Filesystem.new('root') do
            file 'a.txt'
            file 'b.txt'
            directory 'dir' do
              file 'c.txt'
              file 'd.txt'
            end
            file 'e.txt'
          end

          all = fs.all_files
          file_names = all.map { |e| e[:file].name }

          # Files should appear in order: root files first, then directory files
          assert_equal 'a.txt', file_names[0]
          assert_equal 'b.txt', file_names[1]
          assert_equal 'e.txt', file_names[2]
          assert_equal 'c.txt', file_names[3]
          assert_equal 'd.txt', file_names[4]
        end

        def test_attributes_isolation_between_instances
          fs1 = Filesystem.new('root')
          fs2 = Filesystem.new('root')

          fs1.attr(:key, 'value1')
          fs2.attr(:key, 'value2')

          assert_equal 'value1', fs1.attr(:key)
          assert_equal 'value2', fs2.attr(:key)
        end

        def test_children_and_files_are_mutable_arrays
          # While VirtualFile is immutable, the collections should be mutable
          @fs.file('file1.txt')
          assert_equal 1, @fs.files.length

          @fs.file('file2.txt')
          assert_equal 2, @fs.files.length

          @fs.directory('dir1')
          assert_equal 1, @fs.children.length

          @fs.directory('dir2')
          assert_equal 2, @fs.children.length
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
