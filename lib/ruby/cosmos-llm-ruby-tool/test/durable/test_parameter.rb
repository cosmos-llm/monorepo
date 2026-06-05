# frozen_string_literal: true

require 'test_helper'

module Cosmos
  module Llm
    module Tool
      class TestParameter < Minitest::Test
        def test_initialize_defaults
          param = Parameter.new(:test)

          assert_equal :test, param.name
          assert_equal :string, param.type
          assert_equal '', param.description
          refute param.required?
          assert_nil param.enum
          assert_nil param.default
          assert_empty param.properties
        end

        def test_initialize_with_options
          param = Parameter.new(:age,
                                type: :number,
                                description: 'User age',
                                required: true,
                                default: 18)

          assert_equal :age, param.name
          assert_equal :number, param.type
          assert_equal 'User age', param.description
          assert param.required?
          assert_equal 18, param.default
        end

        def test_initialize_symbol_name
          param = Parameter.new(:test)

          assert_equal :test, param.name
        end

        def test_initialize_string_name
          param = Parameter.new('test')

          assert_equal :test, param.name
        end

        def test_initialize_integer_name
          param = Parameter.new(123)

          assert_equal :"123", param.name
        end

        def test_initialize_with_enum
          param = Parameter.new(:option, enum: %w[a b c])

          assert_equal %w[a b c], param.enum
          assert param.enum?
        end

        def test_initialize_with_properties
          param = Parameter.new(:obj, type: :object, properties: { key: 'value' })

          assert_equal({ key: 'value' }, param.properties)
        end

        def test_initialize_invalid_enum_type
          error = assert_raises(ArgumentError) do
            Parameter.new(:test, enum: 'invalid')
          end

          assert_match(/enum must be an Array or nil/, error.message)
        end

        def test_initialize_invalid_properties_type
          error = assert_raises(ArgumentError) do
            Parameter.new(:test, properties: 'invalid')
          end

          assert_match(/properties must be a Hash or nil/, error.message)
        end

        def test_initialize_properties_nil
          param = Parameter.new(:test, properties: nil)

          assert_empty param.properties
        end

        def test_valid_types
          Parameter::VALID_TYPES.each do |type|
            param = Parameter.new(:test, type: type)

            assert_equal type, param.type
          end
        end

        def test_invalid_type
          error = assert_raises(ArgumentError) do
            Parameter.new(:test, type: :invalid)
          end

          assert_match(/Invalid parameter type/, error.message)
          assert_match(/invalid/, error.message)
        end

        def test_required_question_mark_true
          param = Parameter.new(:test, required: true)

          assert param.required?
        end

        def test_required_question_mark_false
          param = Parameter.new(:test, required: false)

          refute param.required?
        end

        def test_enum_question_mark_true
          param = Parameter.new(:test, enum: %w[a b])

          assert param.enum?
        end

        def test_enum_question_mark_false_nil
          param = Parameter.new(:test, enum: nil)

          refute param.enum?
        end

        def test_enum_question_mark_false_empty
          param = Parameter.new(:test, enum: [])

          refute param.enum?
        end

        def test_validate_enum_empty_array
          param = Parameter.new(:test, enum: [])

          # Since enum? is false for empty array, it should not check enum
          assert param.validate('any_value')
        end

        def test_validate_required_success
          param = Parameter.new(:test, required: true)

          assert param.validate('value')
        end

        def test_validate_required_failure
          param = Parameter.new(:test, required: true)

          error = assert_raises(ValidationError) do
            param.validate(nil)
          end

          assert_match(/required/, error.message)
          assert_match(/test/, error.message)
        end

        def test_validate_optional_nil
          param = Parameter.new(:test, required: false)

          assert param.validate(nil)
        end

        def test_validate_string_type_success
          param = Parameter.new(:test, type: :string)

          assert param.validate('hello')
        end

        def test_validate_string_type_failure
          param = Parameter.new(:test, type: :string)

          error = assert_raises(ValidationError) do
            param.validate(123)
          end

          assert_match(/must be of type string/, error.message)
        end

        def test_validate_number_type_success
          param = Parameter.new(:test, type: :number)

          assert param.validate(123)
          assert param.validate(123.45)
        end

        def test_validate_number_type_failure
          param = Parameter.new(:test, type: :number)

          error = assert_raises(ValidationError) do
            param.validate('not a number')
          end

          assert_match(/must be of type number/, error.message)
        end

        def test_validate_integer_type_success
          param = Parameter.new(:test, type: :integer)

          assert param.validate(123)
        end

        def test_validate_integer_type_failure_float
          param = Parameter.new(:test, type: :integer)

          error = assert_raises(ValidationError) do
            param.validate(123.45)
          end

          assert_match(/must be of type integer/, error.message)
        end

        def test_validate_boolean_type_success
          param = Parameter.new(:test, type: :boolean)

          assert param.validate(true)
          assert param.validate(false)
        end

        def test_validate_boolean_type_failure
          param = Parameter.new(:test, type: :boolean)

          error = assert_raises(ValidationError) do
            param.validate('not a boolean')
          end

          assert_match(/must be of type boolean/, error.message)
        end

        def test_validate_array_type_success
          param = Parameter.new(:test, type: :array)

          assert param.validate([1, 2, 3])
          assert param.validate([])
        end

        def test_validate_array_type_failure
          param = Parameter.new(:test, type: :array)

          error = assert_raises(ValidationError) do
            param.validate('not an array')
          end

          assert_match(/must be of type array/, error.message)
        end

        def test_validate_object_type_success
          param = Parameter.new(:test, type: :object)

          assert param.validate({ key: 'value' })
          assert param.validate({})
        end

        def test_validate_object_type_failure
          param = Parameter.new(:test, type: :object)

          error = assert_raises(ValidationError) do
            param.validate('not an object')
          end

          assert_match(/must be of type object/, error.message)
        end

        def test_validate_enum_success
          param = Parameter.new(:test, enum: %w[red green blue])

          assert param.validate('red')
          assert param.validate('green')
          assert param.validate('blue')
        end

        def test_validate_enum_failure
          param = Parameter.new(:test, enum: %w[red green blue])

          error = assert_raises(ValidationError) do
            param.validate('yellow')
          end

          assert_match(/must be one of/, error.message)
          assert_match(/red/, error.message)
        end

        def test_validate_combined_type_and_enum
          param = Parameter.new(:test, type: :string, enum: %w[a b c])

          assert param.validate('a')

          error = assert_raises(ValidationError) do
            param.validate('d')
          end

          assert_match(/must be one of/, error.message)
        end

        def test_to_h_minimal
          param = Parameter.new(:test)

          hash = param.to_h

          assert_equal :test, hash[:name]
          assert_equal :string, hash[:type]
          assert_equal '', hash[:description]
          assert_equal false, hash[:required]
        end

        def test_to_h_full
          param = Parameter.new(:test,
                                type: :number,
                                description: 'A test param',
                                required: true,
                                enum: [1, 2, 3],
                                default: 1,
                                properties: { nested: true })

          hash = param.to_h

          assert_equal :test, hash[:name]
          assert_equal :number, hash[:type]
          assert_equal 'A test param', hash[:description]
          assert_equal true, hash[:required]
          assert_equal [1, 2, 3], hash[:enum]
          assert_equal 1, hash[:default]
          assert_equal({ nested: true }, hash[:properties])
        end

        def test_to_h_compact_nil_values
          param = Parameter.new(:test, enum: nil, default: nil)

          hash = param.to_h

          # Should not include nil values due to .compact
          refute hash.key?(:enum)
          refute hash.key?(:default)
        end

        def test_to_h_includes_empty_properties
          param = Parameter.new(:test, properties: {})

          hash = param.to_h

          assert_equal({}, hash[:properties])
        end

        def test_description_setter
          param = Parameter.new(:test)
          param.description = 'New description'

          assert_equal 'New description', param.description
        end

        def test_name_is_read_only
          param = Parameter.new(:test)

          assert_raises(NoMethodError) do
            param.name = :new_name
          end
        end

        def test_type_is_read_only
          param = Parameter.new(:test)

          assert_raises(NoMethodError) do
            param.type = :number
          end
        end

        def test_required_is_read_only
          param = Parameter.new(:test)

          assert_raises(NoMethodError) do
            param.required = true
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
