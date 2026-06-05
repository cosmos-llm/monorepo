# frozen_string_literal: true

require 'test_helper'

module Cosmos
  module Llm
    module Context
      class TestBlock < Minitest::Test
        def setup
          @block = Block.new(:system, 'Test content')
        end

        def test_initialize
          assert_equal :system, @block.name
          assert_equal 'Test content', @block.content
          assert_empty @block.metadata
        end

        def test_initialize_with_metadata
          block = Block.new(:user, 'Content', { role: 'user', timestamp: 12_345 })

          assert_equal :user, block.name
          assert_equal 'Content', block.content
          assert_equal 'user', block.metadata[:role]
          assert_equal 12_345, block.metadata[:timestamp]
        end

        def test_initialize_with_symbol_name
          block = Block.new(:test, 'content')

          assert_equal :test, block.name
        end

        def test_initialize_with_string_name
          block = Block.new('test', 'content')

          # String names are coerced to symbols
          assert_equal :test, block.name
        end

        def test_initialize_with_hash_content
          block = Block.new(:tool, { name: 'calculator', params: {} })

          assert_instance_of Hash, block.content
          assert_equal 'calculator', block.content[:name]
        end

        def test_initialize_with_array_content
          block = Block.new(:list, [1, 2, 3])

          assert_instance_of Array, block.content
          assert_equal 3, block.content.length
        end

        def test_content_immutable
          # Blocks are immutable - use with_content instead
          new_block = @block.with_content('New content')

          assert_equal 'New content', new_block.content
          assert_equal 'Test content', @block.content # Original unchanged
        end

        def test_meta_get
          block = Block.new(:test, 'content', role: 'system')

          assert_equal 'system', block.meta(:role)
        end

        def test_meta_get_nonexistent
          assert_nil @block.meta(:nonexistent)
        end

        def test_with_metadata
          new_block = @block.with_metadata(timestamp: 67_890)

          assert_equal 67_890, new_block.meta(:timestamp)
          assert_nil @block.meta(:timestamp) # Original unchanged
        end

        def test_with_metadata_multiple_values
          new_block = @block.with_metadata(key1: 'value1', key2: 'value2', key3: 'value3')

          assert_equal 'value1', new_block.meta(:key1)
          assert_equal 'value2', new_block.meta(:key2)
          assert_equal 'value3', new_block.meta(:key3)
        end

        def test_type_question_mark_true
          assert @block.type?(:system)
        end

        def test_type_question_mark_false
          refute @block.type?(:user)
        end

        def test_type_question_mark_string_vs_symbol
          assert @block.type?('system')
        end

        def test_type_question_mark_symbol_vs_string
          block = Block.new('test', 'content')

          assert block.type?(:test)
        end

        def test_to_h
          block = Block.new(:system, 'Test content', role: 'system')
          hash = block.to_h

          assert_equal :system, hash[:name]
          assert_equal 'Test content', hash[:content]
          assert_equal 'system', hash[:metadata][:role]
        end

        def test_to_h_empty_metadata
          hash = @block.to_h

          assert_empty hash[:metadata]
        end

        def test_to_h_complex_content
          block = Block.new(:data, { nested: { key: 'value' } })

          hash = block.to_h

          assert_equal 'value', hash[:content][:nested][:key]
        end

        def test_to_s_short_content
          block = Block.new(:test, 'Short')

          str = block.to_s

          assert_includes str, 'Block:test'
          assert_includes str, 'Short'
        end

        def test_to_s_long_content
          long_content = 'a' * 100
          block = Block.new(:test, long_content)

          str = block.to_s

          assert_includes str, 'Block:test'
          assert_includes str, '...'
          refute_includes str, long_content # Should be truncated
        end

        def test_inspect
          str = @block.inspect

          assert_includes str, 'Cosmos::Llm::Context::Block'
          assert_includes str, '@name=:system'
          assert_includes str, '@content="Test content"'
          assert_includes str, '@metadata={}'
        end

        def test_inspect_with_metadata
          block = Block.new(:test, 'content', key: 'value')
          str = block.inspect

          assert_includes str, '@metadata={key: "value"}'
        end

        def test_blocks_with_different_types
          system_block = Block.new(:system, 'System message')
          user_block = Block.new(:user, 'User message')
          assistant_block = Block.new(:assistant, 'Assistant message')
          tool_block = Block.new(:tool, { name: 'test' })
          string_block = Block.new(:string, 'Plain text')

          assert system_block.type?(:system)
          assert user_block.type?(:user)
          assert assistant_block.type?(:assistant)
          assert tool_block.type?(:tool)
          assert string_block.type?(:string)
        end

        def test_block_immutability_of_name
          # Name should be read-only
          assert_raises(NoMethodError) do
            @block.name = :new_name
          end
        end

        def test_initialize_with_nil_name_raises_error
          assert_raises(InvalidNameError, 'Name cannot be nil') do
            Block.new(nil, 'content')
          end
        end

        def test_initialize_with_empty_string_name_raises_error
          assert_raises(InvalidNameError, 'Name cannot be empty') do
            Block.new('', 'content')
          end
        end

        def test_initialize_with_invalid_name_type_raises_error
          assert_raises(InvalidNameError, 'Name must be a String or Symbol, got Integer') do
            Block.new(123, 'content')
          end
        end

        def test_initialize_with_non_hash_metadata_raises_error
          assert_raises(ValidationError, 'Metadata must be a Hash, got String') do
            Block.new(:test, 'content', 'invalid_metadata')
          end
        end

        def test_initialize_with_array_metadata_raises_error
          assert_raises(ValidationError, 'Metadata must be a Hash, got Array') do
            Block.new(:test, 'content', [])
          end
        end

        def test_equality_with_identical_blocks
          block1 = Block.new(:system, 'content', key: 'value')
          block2 = Block.new(:system, 'content', key: 'value')

          assert_equal block1, block2
          assert block1.eql?(block2)
        end

        def test_equality_with_different_names
          block1 = Block.new(:system, 'content')
          block2 = Block.new(:user, 'content')

          refute_equal block1, block2
          refute block1.eql?(block2)
        end

        def test_equality_with_different_content
          block1 = Block.new(:system, 'content1')
          block2 = Block.new(:system, 'content2')

          refute_equal block1, block2
          refute block1.eql?(block2)
        end

        def test_equality_with_different_metadata
          block1 = Block.new(:system, 'content', key: 'value1')
          block2 = Block.new(:system, 'content', key: 'value2')

          refute_equal block1, block2
          refute block1.eql?(block2)
        end

        def test_equality_with_non_block_object
          block = Block.new(:system, 'content')

          refute_equal block, 'not a block'
          refute_equal block, nil
          refute_equal block, 123
        end

        def test_hash_consistency
          block1 = Block.new(:system, 'content', key: 'value')
          block2 = Block.new(:system, 'content', key: 'value')
          block3 = Block.new(:user, 'content', key: 'value')

          assert_equal block1.hash, block2.hash
          refute_equal block1.hash, block3.hash
        end

        def test_hash_equality_correspondence
          block1 = Block.new(:system, 'content')
          block2 = Block.new(:system, 'content')
          block3 = Block.new(:user, 'content')

          # If blocks are equal, they must have the same hash
          assert_equal block1 == block2, block1.hash == block2.hash
          assert_equal block1 == block3, block1.hash == block3.hash
        end

        def test_with_content_creates_new_block
          original_content = 'original'
          new_content = 'new content'
          block = Block.new(:test, original_content)

          new_block = block.with_content(new_content)

          assert_equal original_content, block.content
          assert_equal new_content, new_block.content
          assert_equal block.name, new_block.name
          assert_equal block.metadata, new_block.metadata
        end

        def test_with_content_with_hash_content
          block = Block.new(:test, 'original')
          new_content = { key: 'value', nested: { data: 123 } }

          new_block = block.with_content(new_content)

          assert_equal new_content, new_block.content
          assert_instance_of Hash, new_block.content
        end

        def test_with_content_with_array_content
          block = Block.new(:test, 'original')
          new_content = [1, 2, 3, 'four']

          new_block = block.with_content(new_content)

          assert_equal new_content, new_block.content
          assert_instance_of Array, new_block.content
        end

        def test_with_content_with_nil_content
          block = Block.new(:test, 'original')

          new_block = block.with_content(nil)

          assert_nil new_block.content
        end

        def test_with_content_preserves_metadata
          block = Block.new(:test, 'original', key1: 'value1', key2: 'value2')

          new_block = block.with_content('new')

          assert_equal 'value1', new_block.meta(:key1)
          assert_equal 'value2', new_block.meta(:key2)
        end

        def test_metadata_is_frozen
          block = Block.new(:test, 'content', key: 'value')

          assert block.metadata.frozen?
        end

        def test_block_is_frozen
          block = Block.new(:test, 'content')

          assert block.frozen?
        end

        def test_with_metadata_preserves_existing_metadata
          block = Block.new(:test, 'content', existing: 'value')
          new_block = block.with_metadata(new_key: 'new_value')

          assert_equal 'value', new_block.meta(:existing)
          assert_equal 'new_value', new_block.meta(:new_key)
        end

        def test_with_metadata_overwrites_existing_keys
          block = Block.new(:test, 'content', key: 'old_value')
          new_block = block.with_metadata(key: 'new_value')

          assert_equal 'new_value', new_block.meta(:key)
          assert_equal 'old_value', block.meta(:key) # Original unchanged
        end

        def test_with_metadata_with_empty_hash
          block = Block.new(:test, 'content', key: 'value')
          new_block = block.with_metadata({})

          assert_equal 'value', new_block.meta(:key)
          assert_equal block.metadata, new_block.metadata
        end

        def test_meta_with_symbol_key
          block = Block.new(:test, 'content', role: 'system')

          assert_equal 'system', block.meta(:role)
        end

        def test_meta_with_string_key
          block = Block.new(:test, 'content', 'role' => 'system')

          assert_equal 'system', block.meta('role')
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
