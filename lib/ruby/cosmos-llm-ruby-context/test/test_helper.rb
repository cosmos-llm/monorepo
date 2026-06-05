# frozen_string_literal: true

$LOAD_PATH.unshift File.expand_path('../lib', __dir__)

# Mock the virtual filesystem module for testing
module Cosmos
  module Llm
    module VirtualFilesystem
      class Filesystem
        attr_reader :name, :files, :children, :attributes

        def initialize(name, &block)
          @name = name
          @files = []
          @children = []
          @attributes = {}
          instance_eval(&block) if block_given?
        end

        def attr(key, value = nil)
          if value.nil?
            @attributes[key]
          else
            @attributes[key] = value
          end
        end

        def tree
          # Simple tree representation for testing
          lines = []
          lines << "#{@name}/"
          @files.each do |file|
            lines << "  #{file.name}"
          end
          @children.each do |child|
            child.tree.split("\n").each do |line|
              lines << "  #{line}"
            end
          end
          lines.join("\n")
        end

        def to_h
          {
            name: @name,
            files: @files.map(&:to_h),
            directories: @children.map(&:to_h),
            attributes: @attributes
          }
        end

        def all_files(prefix = '')
          base_path = prefix.empty? ? @name : "#{prefix}/#{@name}"
          result = @files.map do |file|
            { file: file, path: "#{base_path}/#{file.name}" }
          end
          @children.each do |child|
            child.all_files(base_path).each do |entry|
              result << entry
            end
          end
          result
        end

        def find_file(path)
          return nil if path.nil? || path.empty?

          parts = path.split('/')
          return nil if parts.empty?

          if parts.length == 1
            # File at root
            @files.find { |f| f.name == parts[0] }
          else
            # Nested file
            child_name = parts.shift
            child = @children.find { |c| c.name == child_name }
            return nil unless child

            child.find_file(parts.join('/'))
          end
        end

        def directory(name, &block)
          dir = self.class.new(name)
          @children << dir
          dir.instance_eval(&block) if block_given?
          dir
        end

        def file(name, content: nil, **attributes)
          file_obj = VirtualFile.new(name, content, attributes)
          @files << file_obj
          file_obj
        end
      end

      class VirtualFile
        attr_reader :name, :content, :attributes

        def initialize(name, content = nil, attributes = {})
          @name = name
          @content = content
          @attributes = attributes
        end

        def with_content(new_content)
          self.class.new(@name, new_content, @attributes)
        end

        def to_h
          {
            name: @name,
            content: @content,
            attributes: @attributes
          }
        end
      end
    end
  end
end

# Mock the require for virtual_filesystem
$LOADED_FEATURES << 'cosmos/llm/virtual_filesystem.rb'

require 'cosmos/llm/context'

require 'minitest/autorun'
