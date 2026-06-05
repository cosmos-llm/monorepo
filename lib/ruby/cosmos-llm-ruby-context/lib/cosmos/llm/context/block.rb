# frozen_string_literal: true

module Cosmos
  module Llm
    module Context
      # Represents a named block of content within an LLM context.
      #
      # Context blocks are flexible containers for different types of content that
      # make up an agentic context. They can represent system prompts, user messages,
      # tool declarations, arbitrary strings, or any other structured content.
      #
      # @example Create a system prompt block
      #   block = Block.new(:system, "You are a helpful assistant")
      #
      # @example Create a tool declaration block
      #   block = Block.new(:tool, { name: "calculator", parameters: [...] })
      #
      # @see Builder
      class Block
        # @return [Symbol, String] The name/type of this block
        attr_reader :name

        # @return [Object] The content of this block
        attr_reader :content

        # @return [Hash] Additional metadata for this block
        attr_reader :metadata

        # Initializes a new Block.
        #
        # @param name [Symbol, String] The name/type of the block
        # @param content [Object] The content of the block
        # @param metadata [Hash] Optional metadata for the block
        # @return [Block] A new block instance
        # @raise [InvalidNameError] If name is invalid
        # @raise [ValidationError] If metadata is not a Hash
        def initialize(name, content, metadata = {})
          @name = validate_and_coerce_name(name)
          @content = content
          @metadata = validate_metadata(metadata).freeze
          freeze
        end

        # Gets a metadata value.
        #
        # Since blocks are immutable, metadata cannot be modified after creation.
        # To create a block with different metadata, create a new Block instance.
        #
        # @param key [Symbol, String] The metadata key
        # @return [Object, nil] The metadata value, or nil if not found
        # @example Get metadata
        #   block.meta(:role) # => 'system'
        def meta(key)
          @metadata[key]
        end

        # Creates a new Block with updated metadata.
        #
        # @param updates [Hash] The metadata updates to apply
        # @return [Block] A new Block instance with merged metadata
        # @example Update metadata
        #   new_block = block.with_metadata(role: 'user')
        def with_metadata(updates)
          Block.new(@name, @content, @metadata.merge(updates))
        end

        # Creates a new Block with updated content.
        #
        # @param new_content [Object] The new content
        # @return [Block] A new Block instance with updated content
        # @example Update content
        #   new_block = block.with_content("new content")
        def with_content(new_content)
          Block.new(@name, new_content, @metadata)
        end

        # Checks if this is a specific type of block.
        #
        # @param block_type [Symbol, String] The type to check
        # @return [Boolean] True if the block matches the type
        # @example Check block type
        #   block = Block.new(:system, "...")
        #   block.type?(:system) # => true
        #   block.type?(:user) # => false
        def type?(block_type)
          @name.to_sym == block_type.to_sym
        end

        # Converts the block to a hash representation.
        #
        # @return [Hash] Hash with name, content, and metadata
        def to_h
          {
            name: @name,
            content: @content,
            metadata: @metadata
          }
        end

        # Returns a string representation of the block.
        #
        # @return [String] String representation
        def to_s
          content_preview = @content.to_s.length > 50 ? "#{@content.to_s[0..46]}..." : @content.to_s
          "#<Block:#{@name} content=\"#{content_preview}\">"
        end

        # Inspects the block.
        #
        # @return [String] Detailed string representation
        def inspect
          "#<Cosmos::Llm::Context::Block:0x#{object_id.to_s(16)} " \
            "@name=#{@name.inspect} @content=#{@content.inspect} " \
            "@metadata=#{@metadata.inspect}>"
        end

        # Implements equality comparison.
        #
        # @param other [Object] The object to compare with
        # @return [Boolean] True if the blocks are equal
        def ==(other)
          return false unless other.is_a?(Block)

          name == other.name &&
            content == other.content &&
            metadata == other.metadata
        end

        alias eql? ==

        # Generates a hash code for the block.
        #
        # @return [Integer] The hash code
        def hash
          [name, content, metadata].hash
        end

        private

        # Validates and coerces the name to a Symbol.
        #
        # @param name [Object] The name to validate
        # @return [Symbol] The validated name as a Symbol
        # @raise [InvalidNameError] If the name is invalid
        def validate_and_coerce_name(name)
          raise InvalidNameError, 'Name cannot be nil' if name.nil?

          case name
          when Symbol
            name
          when String
            raise InvalidNameError, 'Name cannot be empty' if name.empty?

            name.to_sym
          else
            raise InvalidNameError, "Name must be a String or Symbol, got #{name.class}"
          end
        end

        # Validates that metadata is a Hash.
        #
        # @param metadata [Object] The metadata to validate
        # @return [Hash] The validated metadata
        # @raise [ValidationError] If metadata is not a Hash
        def validate_metadata(metadata)
          raise ValidationError, "Metadata must be a Hash, got #{metadata.class}" unless metadata.is_a?(Hash)

          metadata
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
