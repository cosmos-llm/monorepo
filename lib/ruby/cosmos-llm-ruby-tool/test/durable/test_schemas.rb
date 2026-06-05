# frozen_string_literal: true

require 'test_helper'

module Cosmos
  module Llm
    module Tool
      class TestSchemasOpenAi < Minitest::Test
        def setup
          @tool = Definition.new(:calculator) do
            description 'Performs basic arithmetic'
            parameter :operation, type: :string, enum: %w[add subtract], required: true
            parameter :a, type: :number, required: true
            parameter :b, type: :number, required: true
          end
        end

        def test_generate_basic_structure
          schema = Schemas::OpenAi.generate(@tool)

          assert_equal 'function', schema[:type]
          assert_instance_of Hash, schema[:function]
        end

        def test_generate_function_name
          schema = Schemas::OpenAi.generate(@tool)

          assert_equal 'calculator', schema[:function][:name]
        end

        def test_generate_function_description
          schema = Schemas::OpenAi.generate(@tool)

          assert_equal 'Performs basic arithmetic', schema[:function][:description]
        end

        def test_generate_parameters_structure
          schema = Schemas::OpenAi.generate(@tool)
          params = schema[:function][:parameters]

          assert_equal 'object', params[:type]
          assert_instance_of Hash, params[:properties]
        end

        def test_generate_required_parameters
          schema = Schemas::OpenAi.generate(@tool)
          required = schema[:function][:parameters][:required]

          assert_instance_of Array, required
          assert_equal 3, required.length
          assert_includes required, 'operation'
          assert_includes required, 'a'
          assert_includes required, 'b'
        end

        def test_generate_parameter_properties
          schema = Schemas::OpenAi.generate(@tool)
          props = schema[:function][:parameters][:properties]

          assert_equal 3, props.length
          assert props.key?('operation')
          assert props.key?('a')
          assert props.key?('b')
        end

        def test_parameter_schema_with_enum
          schema = Schemas::OpenAi.generate(@tool)
          operation = schema[:function][:parameters][:properties]['operation']

          assert_equal 'string', operation[:type]
          assert_equal %w[add subtract], operation[:enum]
        end

        def test_parameter_schema_number_type
          schema = Schemas::OpenAi.generate(@tool)
          a_param = schema[:function][:parameters][:properties]['a']

          assert_equal 'number', a_param[:type]
        end

        def test_parameter_schema_with_description
          tool = Definition.new(:test) do
            parameter :name, type: :string, description: 'User name', required: true
          end

          schema = Schemas::OpenAi.generate(tool)
          name_param = schema[:function][:parameters][:properties]['name']

          assert_equal 'User name', name_param[:description]
        end

        def test_parameter_schema_with_default
          tool = Definition.new(:test) do
            parameter :value, type: :number, default: 10
          end

          schema = Schemas::OpenAi.generate(tool)
          value_param = schema[:function][:parameters][:properties]['value']

          assert_equal 10, value_param[:default]
        end

        def test_parameter_schema_with_nil_default
          tool = Definition.new(:test) do
            parameter :value, type: :number, default: nil
          end

          schema = Schemas::OpenAi.generate(tool)
          value_param = schema[:function][:parameters][:properties]['value']

          refute value_param.key?(:default)
        end

        def test_parameter_schema_with_empty_description
          tool = Definition.new(:test) do
            parameter :name, type: :string, description: ''
          end

          schema = Schemas::OpenAi.generate(tool)
          name_param = schema[:function][:parameters][:properties]['name']

          refute name_param.key?(:description)
        end

        def test_json_schema_type_conversions
          assert_equal 'string', Schemas::OpenAi.json_schema_type(:string)
          assert_equal 'number', Schemas::OpenAi.json_schema_type(:number)
          assert_equal 'integer', Schemas::OpenAi.json_schema_type(:integer)
          assert_equal 'boolean', Schemas::OpenAi.json_schema_type(:boolean)
          assert_equal 'array', Schemas::OpenAi.json_schema_type(:array)
          assert_equal 'object', Schemas::OpenAi.json_schema_type(:object)
        end

        def test_json_schema_type_unknown_raises_error
          assert_raises(ArgumentError, 'Unknown parameter type: :unknown') do
            Schemas::OpenAi.json_schema_type(:unknown)
          end
        end

        def test_no_required_parameters
          tool = Definition.new(:optional_tool) do
            parameter :opt1, type: :string
            parameter :opt2, type: :number
          end

          schema = Schemas::OpenAi.generate(tool)

          refute schema[:function][:parameters].key?(:required)
        end

        def test_tool_with_no_parameters
          tool = Definition.new(:no_params_tool) do
            description 'A tool with no parameters'
          end

          schema = Schemas::OpenAi.generate(tool)

          assert_equal 'function', schema[:type]
          assert_equal 'no_params_tool', schema[:function][:name]
          assert_equal 'A tool with no parameters', schema[:function][:description]
          assert_equal 'object', schema[:function][:parameters][:type]
          assert_empty schema[:function][:parameters][:properties]
          refute schema[:function][:parameters].key?(:required)
        end
      end

      class TestSchemasAnthropic < Minitest::Test
        def setup
          @tool = Definition.new(:weather) do
            description 'Get weather information'
            parameter :location, type: :string, required: true, description: 'City name'
            parameter :units, type: :string, enum: %w[celsius fahrenheit]
          end
        end

        def test_generate_basic_structure
          schema = Schemas::Anthropic.generate(@tool)

          assert_instance_of Hash, schema
          assert schema.key?(:name)
          assert schema.key?(:description)
          assert schema.key?(:input_schema)
        end

        def test_generate_name
          schema = Schemas::Anthropic.generate(@tool)

          assert_equal 'weather', schema[:name]
        end

        def test_generate_description
          schema = Schemas::Anthropic.generate(@tool)

          assert_equal 'Get weather information', schema[:description]
        end

        def test_generate_input_schema_structure
          schema = Schemas::Anthropic.generate(@tool)
          input_schema = schema[:input_schema]

          assert_equal 'object', input_schema[:type]
          assert_instance_of Hash, input_schema[:properties]
        end

        def test_generate_required_parameters
          schema = Schemas::Anthropic.generate(@tool)
          required = schema[:input_schema][:required]

          assert_instance_of Array, required
          assert_includes required, 'location'
          refute_includes required, 'units'
        end

        def test_parameter_properties_use_string_keys
          schema = Schemas::Anthropic.generate(@tool)
          props = schema[:input_schema][:properties]

          assert props.key?('location')
          assert props.key?('units')
        end

        def test_parameter_schema_with_description
          schema = Schemas::Anthropic.generate(@tool)
          location = schema[:input_schema][:properties]['location']

          assert_equal 'City name', location[:description]
        end

        def test_parameter_schema_with_enum
          schema = Schemas::Anthropic.generate(@tool)
          units = schema[:input_schema][:properties]['units']

          assert_equal %w[celsius fahrenheit], units[:enum]
        end

        def test_parameter_schema_no_default
          schema = Schemas::Anthropic.generate(@tool)
          location = schema[:input_schema][:properties]['location']

          # Anthropic schema doesn't include default values
          refute location.key?(:default)
        end

        def test_parameter_schema_with_empty_description
          tool = Definition.new(:test) do
            parameter :name, type: :string, description: ''
          end

          schema = Schemas::Anthropic.generate(tool)
          name_param = schema[:input_schema][:properties]['name']

          refute name_param.key?(:description)
        end

        def test_json_schema_type_conversions
          assert_equal 'string', Schemas::Anthropic.json_schema_type(:string)
          assert_equal 'number', Schemas::Anthropic.json_schema_type(:number)
          assert_equal 'integer', Schemas::Anthropic.json_schema_type(:integer)
          assert_equal 'boolean', Schemas::Anthropic.json_schema_type(:boolean)
          assert_equal 'array', Schemas::Anthropic.json_schema_type(:array)
          assert_equal 'object', Schemas::Anthropic.json_schema_type(:object)
        end

        def test_json_schema_type_unknown_raises_error
          assert_raises(ArgumentError, 'Unknown parameter type: :unknown') do
            Schemas::Anthropic.json_schema_type(:unknown)
          end
        end

        def test_all_parameter_types
          tool = Definition.new(:types_test) do
            parameter :str, type: :string
            parameter :num, type: :number
            parameter :int, type: :integer
            parameter :bool, type: :boolean
            parameter :arr, type: :array
            parameter :obj, type: :object
          end

          schema = Schemas::Anthropic.generate(tool)
          props = schema[:input_schema][:properties]

          assert_equal 'string', props['str'][:type]
          assert_equal 'number', props['num'][:type]
          assert_equal 'integer', props['int'][:type]
          assert_equal 'boolean', props['bool'][:type]
          assert_equal 'array', props['arr'][:type]
          assert_equal 'object', props['obj'][:type]
        end

        def test_tool_with_no_parameters
          tool = Definition.new(:no_params_tool) do
            description 'A tool with no parameters'
          end

          schema = Schemas::Anthropic.generate(tool)

          assert_equal 'no_params_tool', schema[:name]
          assert_equal 'A tool with no parameters', schema[:description]
          assert_equal 'object', schema[:input_schema][:type]
          assert_empty schema[:input_schema][:properties]
          refute schema[:input_schema].key?(:required)
        end
      end

      class TestSchemasJsonSchema < Minitest::Test
        def setup
          @tool = Definition.new(:calculator) do
            description 'A simple calculator'
            parameter :a, type: :number, required: true, description: 'First number'
            parameter :b, type: :number, required: true, description: 'Second number'
            parameter :operation, type: :string, enum: %w[add subtract], default: 'add'
          end
        end

        def test_generate_basic_structure
          schema = Schemas::JsonSchema.generate(@tool)

          assert_equal 'http://json-schema.org/draft-07/schema#', schema[:'$schema']
          assert_equal 'object', schema[:type]
        end

        def test_generate_title
          schema = Schemas::JsonSchema.generate(@tool)

          assert_equal 'calculator', schema[:title]
        end

        def test_generate_description
          schema = Schemas::JsonSchema.generate(@tool)

          assert_equal 'A simple calculator', schema[:description]
        end

        def test_generate_properties
          schema = Schemas::JsonSchema.generate(@tool)
          props = schema[:properties]

          assert_instance_of Hash, props
          assert_equal 3, props.length
          assert props.key?('a')
          assert props.key?('b')
          assert props.key?('operation')
        end

        def test_generate_required
          schema = Schemas::JsonSchema.generate(@tool)

          assert_instance_of Array, schema[:required]
          assert_equal 2, schema[:required].length
          assert_includes schema[:required], 'a'
          assert_includes schema[:required], 'b'
          refute_includes schema[:required], 'operation'
        end

        def test_parameter_schema_with_description
          schema = Schemas::JsonSchema.generate(@tool)
          a_param = schema[:properties]['a']

          assert_equal 'First number', a_param[:description]
        end

        def test_parameter_schema_with_enum
          schema = Schemas::JsonSchema.generate(@tool)
          operation = schema[:properties]['operation']

          assert_equal %w[add subtract], operation[:enum]
        end

        def test_parameter_schema_with_default
          schema = Schemas::JsonSchema.generate(@tool)
          operation = schema[:properties]['operation']

          assert_equal 'add', operation[:default]
        end

        def test_parameter_schema_with_nil_default
          tool = Definition.new(:test) do
            parameter :value, type: :number, default: nil
          end

          schema = Schemas::JsonSchema.generate(tool)
          value_param = schema[:properties]['value']

          refute value_param.key?(:default)
        end

        def test_parameter_schema_with_empty_description
          tool = Definition.new(:test) do
            parameter :name, type: :string, description: ''
          end

          schema = Schemas::JsonSchema.generate(tool)
          name_param = schema[:properties]['name']

          refute name_param.key?(:description)
        end

        def test_parameter_schema_with_object_properties
          tool = Definition.new(:test) do
            parameter :config,
                      type: :object,
                      properties: {
                        'timeout' => { 'type' => 'number' },
                        'retry' => { 'type' => 'boolean' }
                      }
          end

          schema = Schemas::JsonSchema.generate(tool)
          config = schema[:properties]['config']

          assert_equal 'object', config[:type]
          assert config.key?(:properties)
          assert config[:properties].key?('timeout')
          assert config[:properties].key?('retry')
        end

        def test_json_schema_type_conversions
          assert_equal 'string', Schemas::JsonSchema.json_schema_type(:string)
          assert_equal 'number', Schemas::JsonSchema.json_schema_type(:number)
          assert_equal 'integer', Schemas::JsonSchema.json_schema_type(:integer)
          assert_equal 'boolean', Schemas::JsonSchema.json_schema_type(:boolean)
          assert_equal 'array', Schemas::JsonSchema.json_schema_type(:array)
          assert_equal 'object', Schemas::JsonSchema.json_schema_type(:object)
        end

        def test_json_schema_type_unknown_raises_error
          assert_raises(ArgumentError, 'Unknown parameter type: :unknown') do
            Schemas::JsonSchema.json_schema_type(:unknown)
          end
        end

        def test_no_required_parameters
          tool = Definition.new(:optional_tool) do
            parameter :opt1, type: :string
            parameter :opt2, type: :number
          end

          schema = Schemas::JsonSchema.generate(tool)

          refute schema.key?(:required)
        end

        def test_tool_with_no_parameters
          tool = Definition.new(:no_params_tool) do
            description 'A tool with no parameters'
          end

          schema = Schemas::JsonSchema.generate(tool)

          assert_equal 'http://json-schema.org/draft-07/schema#', schema[:'$schema']
          assert_equal 'object', schema[:type]
          assert_equal 'no_params_tool', schema[:title]
          assert_equal 'A tool with no parameters', schema[:description]
          assert_empty schema[:properties]
          refute schema.key?(:required)
        end

        def test_comprehensive_schema
          tool = Definition.new(:comprehensive) do
            description 'A comprehensive test tool'
            parameter :name, type: :string, required: true, description: 'User name'
            parameter :age, type: :integer, description: 'User age', default: 18
            parameter :active, type: :boolean, default: true
            parameter :tags, type: :array
            parameter :meta, type: :object
          end

          schema = Schemas::JsonSchema.generate(tool)

          # Check structure
          assert_equal 'http://json-schema.org/draft-07/schema#', schema[:'$schema']
          assert_equal 'comprehensive', schema[:title]
          assert_equal 'A comprehensive test tool', schema[:description]

          # Check properties
          assert_equal 5, schema[:properties].length

          # Check required
          assert_equal ['name'], schema[:required]

          # Check defaults
          assert_equal 18, schema[:properties]['age'][:default]
          assert_equal true, schema[:properties]['active'][:default]
        end
      end

      class TestSchemasIntegration < Minitest::Test
        def setup
          @tool = Definition.new(:file_manager) do
            description 'Manages files in the system'
            parameter :action, type: :string, enum: %w[read write delete], required: true
            parameter :path, type: :string, required: true, description: 'File path'
            parameter :content, type: :string, description: 'File content (for write action)'
            parameter :append, type: :boolean, default: false
          end
        end

        def test_all_schemas_generate_successfully
          openai_schema = Schemas::OpenAi.generate(@tool)
          anthropic_schema = Schemas::Anthropic.generate(@tool)
          json_schema = Schemas::JsonSchema.generate(@tool)

          refute_nil openai_schema
          refute_nil anthropic_schema
          refute_nil json_schema
        end

        def test_schemas_have_consistent_parameter_count
          openai_schema = Schemas::OpenAi.generate(@tool)
          anthropic_schema = Schemas::Anthropic.generate(@tool)
          json_schema = Schemas::JsonSchema.generate(@tool)

          openai_props = openai_schema[:function][:parameters][:properties]
          anthropic_props = anthropic_schema[:input_schema][:properties]
          json_props = json_schema[:properties]

          assert_equal 4, openai_props.length
          assert_equal 4, anthropic_props.length
          assert_equal 4, json_props.length
        end

        def test_schemas_have_consistent_required_fields
          openai_schema = Schemas::OpenAi.generate(@tool)
          anthropic_schema = Schemas::Anthropic.generate(@tool)
          json_schema = Schemas::JsonSchema.generate(@tool)

          openai_required = openai_schema[:function][:parameters][:required]
          anthropic_required = anthropic_schema[:input_schema][:required]
          json_required = json_schema[:required]

          assert_equal 2, openai_required.length
          assert_equal 2, anthropic_required.length
          assert_equal 2, json_required.length
        end

        def test_tool_method_delegates_correctly
          openai_via_tool = @tool.to_openai_schema
          anthropic_via_tool = @tool.to_anthropic_schema
          json_via_tool = @tool.to_json_schema

          openai_direct = Schemas::OpenAi.generate(@tool)
          anthropic_direct = Schemas::Anthropic.generate(@tool)
          json_direct = Schemas::JsonSchema.generate(@tool)

          assert_equal openai_direct, openai_via_tool
          assert_equal anthropic_direct, anthropic_via_tool
          assert_equal json_direct, json_via_tool
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
