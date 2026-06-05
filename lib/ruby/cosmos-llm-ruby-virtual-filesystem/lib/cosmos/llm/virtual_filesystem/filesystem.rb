# frozen_string_literal: true

module Cosmos
  module Llm
    module VirtualFilesystem
      # Represents a virtual filesystem for LLM contexts.
      #
      # This class provides a hierarchical structure for organizing files and directories
      # within an agentic context. It supports nested directories, file content, and
      # metadata attributes.
      #
      # @example Create a filesystem structure
      #   fs = Filesystem.new('/') do
      #     directory 'src' do
      #       file 'main.rb', content: 'puts "Hello"'
      #       directory 'lib' do
      #         file 'helper.rb', content: 'def help; end'
      #       end
      #     end
      #   end
      #
      class Filesystem
        # @return [String] The name of this filesystem node
        attr_reader :name

        # @return [Array<Filesystem>] Child directories
        attr_reader :children

        # @return [Array<VirtualFile>] Files in this directory
        attr_reader :files

        # @return [Hash] Metadata attributes for this node
        attr_reader :attributes

        # Initializes a new Filesystem node.
        #
        # @param name [String] The name of the directory
        # @yield Optional block to configure the filesystem
        # @return [Filesystem] A new filesystem instance
        def initialize(name, &block)
          @name = name
          @children = []
          @files = []
          @attributes = {}
          instance_eval(&block) if block_given?
        end

        # Adds a child directory to this filesystem node.
        #
        # @param name [String] The name of the directory
        # @yield Block to configure the directory
        # @return [Filesystem] The created directory
        # @example Add a directory
        #   directory 'src' do
        #     file 'main.rb'
        #   end
        def directory(name, &block)
          dir = Filesystem.new(name, &block)
          @children << dir
          dir
        end

        # Adds a file to this filesystem node.
        #
        # @param name [String] The name of the file
        # @param content [String, nil] The content of the file
        # @param attributes [Hash] Additional attributes for the file
        # @return [VirtualFile] The created file
        # @example Add a file with content
        #   file 'config.yml', content: 'debug: true'
        # @example Add a file with attributes
        #   file 'script.rb', content: 'puts "hi"', attributes: { executable: true }
        def file(name, content: nil, **attributes)
          virtual_file = VirtualFile.new(name, content, attributes)
          @files << virtual_file
          virtual_file
        end

        # Sets or gets an attribute on this filesystem node.
        #
        # @param key [Symbol, String] The attribute key
        # @param value [Object, nil] The attribute value (if setting)
        # @return [Object] The attribute value
        # @example Set an attribute
        #   attr :permissions, '0755'
        # @example Get an attribute
        #   attr(:permissions) # => '0755'
        def attr(key, value = nil)
          if value.nil?
            @attributes[key]
          else
            @attributes[key] = value
          end
        end

        # Finds a file by path within the filesystem.
        #
        # @param path [String] The path to the file (e.g., 'src/main.rb')
        # @return [VirtualFile, nil] The file if found, nil otherwise
        def find_file(path)
          parts = path.split('/').reject(&:empty?)
          return nil if parts.empty?

          if parts.length == 1
            @files.find { |f| f.name == parts[0] }
          else
            dir = @children.find { |c| c.name == parts[0] }
            dir&.find_file(parts[1..].join('/'))
          end
        end

        # Lists all files recursively within this filesystem.
        #
        # @param prefix [String] Path prefix for building full paths
        # @return [Array<Hash>] Array of hashes with :path and :file keys
        def all_files(prefix = '')
          current_prefix = prefix.empty? ? name : "#{prefix}/#{name}"
          result = @files.map { |f| { path: "#{current_prefix}/#{f.name}", file: f } }

          @children.each do |child|
            result.concat(child.all_files(current_prefix))
          end

          result
        end

        # Converts the filesystem to a hash representation.
        #
        # @return [Hash] Hash with name, files, children, and attributes
        def to_h
          {
            name: @name,
            files: @files.map(&:to_h),
            directories: @children.map(&:to_h),
            attributes: @attributes
          }
        end

        # Renders the filesystem as a tree structure.
        #
        # @param indent [Integer] Current indentation level
        # @return [String] Tree representation of the filesystem
        def tree(indent = 0)
          result = "#{' ' * indent}#{@name}/\n"

          @files.each do |file|
            result += "#{' ' * (indent + 2)}#{file.name}\n"
          end

          @children.each do |child|
            result += child.tree(indent + 2)
          end

          result
        end

        # Implements equality comparison.
        #
        # @param other [Object] The object to compare with
        # @return [Boolean] True if the filesystems are equal
        def ==(other)
          return false unless other.is_a?(Filesystem)

          name == other.name &&
            children == other.children &&
            files == other.files &&
            attributes == other.attributes
        end

        alias eql? ==

        # Generates a hash code for the filesystem.
        #
        # @return [Integer] The hash code
        def hash
          [name, children, files, attributes].hash
        end
      end

      # Represents a virtual file within a Filesystem.
      #
      # This class encapsulates file content and metadata within the virtual filesystem.
      #
      # @see Filesystem
      class VirtualFile
        # @return [String] The name of the file
        attr_reader :name

        # @return [String, nil] The content of the file
        attr_reader :content

        # @return [Hash] Metadata attributes for the file
        attr_reader :attributes

        # Initializes a new VirtualFile.
        #
        # @param name [String] The file name
        # @param content [String, nil] The file content
        # @param attributes [Hash] Additional attributes
        # @return [VirtualFile] A new virtual file instance
        # @raise [InvalidNameError] If name is invalid
        # @raise [ValidationError] If attributes is not a Hash
        def initialize(name, content = nil, attributes = {})
          @name = validate_filename(name)
          @content = content
          @attributes = validate_attributes(attributes).freeze
          freeze
        end

        # Creates a new VirtualFile with updated content.
        #
        # @param new_content [String, nil] The new content
        # @return [VirtualFile] A new VirtualFile instance
        def with_content(new_content)
          VirtualFile.new(@name, new_content, @attributes)
        end

        # Creates a new VirtualFile with updated attributes.
        #
        # @param updates [Hash] Attribute updates to merge
        # @return [VirtualFile] A new VirtualFile instance
        def with_attributes(updates)
          VirtualFile.new(@name, @content, @attributes.merge(updates))
        end

        # Converts the file to a hash representation.
        #
        # @return [Hash] Hash with name, content, and attributes
        def to_h
          {
            name: @name,
            content: @content,
            attributes: @attributes
          }
        end

        # Implements equality comparison.
        #
        # @param other [Object] The object to compare with
        # @return [Boolean] True if the files are equal
        def ==(other)
          return false unless other.is_a?(VirtualFile)

          name == other.name &&
            content == other.content &&
            attributes == other.attributes
        end

        alias eql? ==

        # Generates a hash code for the file.
        #
        # @return [Integer] The hash code
        def hash
          [name, content, attributes].hash
        end

        private

        # Validates a filename.
        #
        # @param name [Object] The name to validate
        # @return [String] The validated name
        # @raise [InvalidNameError] If the name is invalid
        def validate_filename(name)
          raise InvalidNameError, "Filename cannot be nil" if name.nil?
          raise InvalidNameError, "Filename must be a String, got #{name.class}" unless name.is_a?(String)
          raise InvalidNameError, "Filename cannot be empty" if name.empty?
          raise InvalidPathError, "Filename cannot contain path separators" if name.include?('/')
          raise InvalidPathError, "Filename cannot contain null bytes" if name.include?("\x00")

          name
        end

        # Validates attributes.
        #
        # @param attributes [Object] The attributes to validate
        # @return [Hash] The validated attributes
        # @raise [ValidationError] If attributes is not a Hash
        def validate_attributes(attributes)
          unless attributes.is_a?(Hash)
            raise ValidationError, "Attributes must be a Hash, got #{attributes.class}"
          end

          attributes
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
