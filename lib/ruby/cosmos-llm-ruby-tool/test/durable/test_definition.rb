# frozen_string_literal: true

require 'test_helper'
require 'mocha/minitest'

module Cosmos
  module Llm
    module Tool
      class TestDefinition < Minitest::Test
        def setup
          @tool = Definition.new(:test_tool)
        end

        def test_initialize
          assert_equal :test_tool, @tool.name
          assert_equal '', @tool.description
          assert_empty @tool.parameters
          assert_nil @tool.handler
        end

        def test_initialize_with_block
          tool = Definition.new(:calculator) do
            description 'A calculator'
            parameter :a, type: :number
          end

          assert_equal 'A calculator', tool.description
          assert_equal 1, tool.parameters.length
        end

        def test_initialize_symbol_name
          tool = Definition.new(:test)

          assert_equal :test, tool.name
        end

        def test_initialize_string_name
          tool = Definition.new('test')

          assert_equal :test, tool.name
        end

        def test_description_setter
          @tool.description('Test description')

          assert_equal 'Test description', @tool.description
        end

        def test_description_getter
          @tool.description = 'Direct assignment'

          assert_equal 'Direct assignment', @tool.description
        end

        def test_add_parameter
          param = @tool.parameter(:name, type: :string, required: true)

          assert_equal 1, @tool.parameters.length
          assert_instance_of Parameter, param
          assert_equal :name, param.name
        end

        def test_add_multiple_parameters
          @tool.parameter(:a, type: :number)
          @tool.parameter(:b, type: :number)
          @tool.parameter(:operation, type: :string)

          assert_equal 3, @tool.parameters.length
        end

        def test_parameter_with_options
          param = @tool.parameter(:option,
                                  type: :string,
                                  required: true,
                                  enum: %w[a b c],
                                  description: 'Choose an option')

          assert_equal :string, param.type
          assert param.required?
          assert_equal %w[a b c], param.enum
          assert_equal 'Choose an option', param.description
        end

        def test_execute_setter
          handler = proc { |params| params[:a] + params[:b] }
          @tool.execute(&handler)

          assert_equal handler, @tool.handler
        end

        def test_execute_getter
          handler = proc { 'test' }
          @tool.handler = handler

          assert_equal handler, @tool.execute
        end

        def test_call_delegates_to_executor
          @tool.parameter(:value, type: :number, required: true)
          @tool.execute { |params| params[:value] * 2 }

          Executor.expects(:execute).with(@tool, { value: 5 }).returns(10)

          result = @tool.call(value: 5)

          assert_equal 10, result
        end

        def test_to_openai_schema_delegates
          Schemas::OpenAi.expects(:generate).with(@tool).returns({ type: 'function' })

          result = @tool.to_openai_schema

          assert_equal({ type: 'function' }, result)
        end

        def test_to_anthropic_schema_delegates
          Schemas::Anthropic.expects(:generate).with(@tool).returns({ name: 'test' })

          result = @tool.to_anthropic_schema

          assert_equal({ name: 'test' }, result)
        end

        def test_to_json_schema_delegates
          Schemas::JsonSchema.expects(:generate).with(@tool).returns({ '$schema': 'http://...' })

          result = @tool.to_json_schema

          assert_equal({ '$schema': 'http://...' }, result)
        end

        def test_to_h
          @tool.description('Test tool')
          @tool.parameter(:a, type: :number)
          @tool.execute { |p| p[:a] }

          hash = @tool.to_h

          assert_equal :test_tool, hash[:name]
          assert_equal 'Test tool', hash[:description]
          assert_equal 1, hash[:parameters].length
          assert_equal true, hash[:has_handler]
        end

        def test_to_h_without_handler
          hash = @tool.to_h

          assert_equal false, hash[:has_handler]
        end

        def test_to_s
          str = @tool.to_s

          assert_includes str, 'Tool:test_tool'
          assert_includes str, 'params=0'
        end

        def test_to_s_with_parameters
          @tool.parameter(:a, type: :number)
          @tool.parameter(:b, type: :number)

          str = @tool.to_s

          assert_includes str, 'params=2'
        end

        def test_inspect
          str = @tool.inspect

          assert_includes str, 'Cosmos::Llm::Tool::Definition'
          assert_includes str, '@name=:test_tool'
          assert_includes str, '@parameters=[]'
        end

        def test_complex_tool_definition
          tool = Definition.new(:weather) do
            description 'Get weather information for a location'

            parameter :location, type: :string, required: true, description: 'City name'
            parameter :units, type: :string, enum: %w[celsius fahrenheit], default: 'celsius'
            parameter :include_forecast, type: :boolean, default: false

            execute do |params|
              {
                location: params[:location],
                temperature: 22,
                units: params[:units],
                forecast: params[:include_forecast] ? %w[sunny cloudy] : nil
              }
            end
          end

          assert_equal :weather, tool.name
          assert_equal 'Get weather information for a location', tool.description
          assert_equal 3, tool.parameters.length
          refute_nil tool.handler

          # Check parameters
          location_param = tool.parameters.find { |p| p.name == :location }
          assert location_param.required?

          units_param = tool.parameters.find { |p| p.name == :units }
          assert_equal 'celsius', units_param.default
          assert_equal %w[celsius fahrenheit], units_param.enum
        end

        def test_name_is_read_only
          assert_raises(NoMethodError) do
            @tool.name = :new_name
          end
        end

        def test_parameters_array_is_read_only
          assert_raises(NoMethodError) do
            @tool.parameters = []
          end
        end

        # Edge case tests
        def test_initialize_with_nil_name
          assert_raises(ArgumentError) do
            Definition.new(nil)
          end
        end

        def test_initialize_with_empty_string_name
          tool = Definition.new('')
          assert_equal :'',
                       tool.name
        end

        def test_initialize_with_numeric_name
          tool = Definition.new(123)
          assert_equal :'123', tool.name
        end

        def test_description_with_nil
          @tool.description('test')
          result = @tool.description(nil)
          assert_equal 'test', result
        end

        def test_description_with_empty_string
          @tool.description('')
          assert_equal '', @tool.description
        end

        def test_description_chaining
          result = @tool.description('chained')
          assert_equal 'chained', result
          assert_equal 'chained', @tool.description
        end

        def test_execute_without_block
          handler = proc { 'test' }
          @tool.handler = handler

          result = @tool.execute
          assert_equal handler, result
        end

        def test_execute_with_nil_block
          @tool.execute(nil)
          assert_nil @tool.handler
        end

        def test_parameter_with_invalid_type
          assert_raises(ArgumentError) do
            @tool.parameter(:test, type: :invalid_type)
          end
        end

        def test_parameter_with_nil_name
          assert_raises(ArgumentError) do
            @tool.parameter(nil)
          end
        end

        def test_parameter_with_empty_name
          param = @tool.parameter('')
          assert_equal :'', param.name
        end

        def test_parameter_with_duplicate_names
          @tool.parameter(:dup, type: :string)
          @tool.parameter(:dup, type: :number)

          params = @tool.parameters.select { |p| p.name == :dup }
          assert_equal 2, params.length
          assert_equal :string, params.first.type
          assert_equal :number, params.last.type
        end

        def test_call_without_handler
          @tool.parameter(:test, type: :string, required: true)

          assert_raises(Cosmos::Llm::Tool::NoHandlerError) do
            @tool.call(test: 'value')
          end
        end

        def test_call_with_invalid_parameters
          @tool.parameter(:number_param, type: :number, required: true)
          @tool.execute { |params| params[:number_param] * 2 }

          assert_raises(Cosmos::Llm::Tool::ValidationError) do
            @tool.call(number_param: 'not_a_number')
          end
        end

        def test_call_with_missing_required_parameters
          @tool.parameter(:required_param, type: :string, required: true)
          @tool.execute { |params| "Hello #{params[:required_param]}" }

          assert_raises(Cosmos::Llm::Tool::ValidationError) do
            @tool.call({})
          end
        end

        def test_call_with_execution_error
          @tool.execute { raise 'execution failed' }

          assert_raises(Cosmos::Llm::Tool::ExecutionError) do
            @tool.call({})
          end
        end

        def test_call_with_string_keys
          @tool.parameter(:test_param, type: :string)
          @tool.execute { |params| params[:test_param] }

          result = @tool.call('test_param' => 'value')
          assert_equal 'value', result
        end

        def test_call_with_mixed_key_types
          @tool.parameter(:symbol_key, type: :string)
          @tool.parameter(:string_key, type: :number)
          @tool.execute { |params| "#{params[:symbol_key]}:#{params[:string_key]}" }

          result = @tool.call(symbol_key: 'test', 'string_key' => 42)
          assert_equal 'test:42', result
        end

        def test_call_with_defaults_applied
          @tool.parameter(:optional, type: :string, default: 'default_value')
          @tool.execute { |params| params[:optional] }

          result = @tool.call({})
          assert_equal 'default_value', result
        end

        def test_call_with_enum_validation
          @tool.parameter(:choice, type: :string, enum: %w[a b c])
          @tool.execute { |params| params[:choice] }

          assert_raises(Cosmos::Llm::Tool::ValidationError) do
            @tool.call(choice: 'd')
          end

          result = @tool.call(choice: 'b')
          assert_equal 'b', result
        end

        def test_to_h_with_complex_parameters
          @tool.description('Complex tool')
          @tool.parameter(:str, type: :string, required: true, description: 'A string')
          @tool.parameter(:num, type: :number, default: 42, enum: [1, 42, 100])
          @tool.parameter(:bool, type: :boolean, required: false)
          @tool.execute { |p| p }

          hash = @tool.to_h

          assert_equal :test_tool, hash[:name]
          assert_equal 'Complex tool', hash[:description]
          assert_equal true, hash[:has_handler]
          assert_equal 3, hash[:parameters].length

          str_param = hash[:parameters].find { |p| p[:name] == :str }
          assert_equal :string, str_param[:type]
          assert str_param[:required]
          assert_equal 'A string', str_param[:description]

          num_param = hash[:parameters].find { |p| p[:name] == :num }
          assert_equal :number, num_param[:type]
          assert_equal 42, num_param[:default]
          assert_equal [1, 42, 100], num_param[:enum]
        end

        def test_to_s_with_no_parameters
          str = @tool.to_s
          assert_includes str, 'Tool:test_tool'
          assert_includes str, 'params=0'
        end

        def test_to_s_with_many_parameters
          5.times { |i| @tool.parameter("param#{i}".to_sym, type: :string) }

          str = @tool.to_s
          assert_includes str, 'params=5'
        end

        def test_inspect_with_handler
          @tool.execute { 'test' }

          inspect_str = @tool.inspect
          assert_includes inspect_str, 'Cosmos::Llm::Tool::Definition'
          assert_includes inspect_str, '@name=:test_tool'
          assert_includes inspect_str, '@parameters=[]'
        end

        def test_inspect_with_parameters
          @tool.parameter(:test, type: :string)

          inspect_str = @tool.inspect
          assert_includes inspect_str, '@parameters='
        end

        # DSL integration tests
        def test_dsl_method_order_independence
          tool = Definition.new(:ordered) do
            execute { |p| p[:b] + p[:a] }
            parameter :a, type: :number
            description 'Order test'
            parameter :b, type: :number
          end

          assert_equal 'Order test', tool.description
          assert_equal 2, tool.parameters.length
          refute_nil tool.handler
        end

        def test_dsl_multiple_descriptions
          tool = Definition.new(:multi_desc) do
            description 'First'
            description 'Second'
          end

          assert_equal 'Second', tool.description
        end

        def test_dsl_multiple_executes
          tool = Definition.new(:multi_exec) do
            execute { 'first' }
            execute { 'second' }
          end

          result = tool.handler.call({})
          assert_equal 'second', result
        end

        def test_dsl_complex_nested_execution
          tool = Definition.new(:complex) do
            description 'Complex calculation tool'

            parameter :operation, type: :string, enum: %w[add multiply], required: true
            parameter :a, type: :number, required: true
            parameter :b, type: :number, required: true
            parameter :scale, type: :number, default: 1.0

            execute do |params|
              result = case params[:operation]
                       when 'add'
                         params[:a] + params[:b]
                       when 'multiply'
                         params[:a] * params[:b]
                       else
                         raise 'Invalid operation'
                       end

              result * params[:scale]
            end
          end

          # Test addition
          result = tool.call(operation: 'add', a: 5, b: 3)
          assert_equal 8.0, result

          # Test multiplication with scale
          result = tool.call(operation: 'multiply', a: 4, b: 5, scale: 2.0)
          assert_equal 40.0, result

          # Test with defaults
          result = tool.call(operation: 'add', a: 1, b: 2)
          assert_equal 3.0, result
        end

        # Schema generation tests (without mocks)
        def test_to_openai_schema_basic
          @tool.description('Test tool')
          @tool.parameter(:input, type: :string, required: true, description: 'Input text')

          schema = @tool.to_openai_schema

          assert_equal 'function', schema[:type]
          assert_equal 'test_tool', schema[:function][:name]
          assert_equal 'Test tool', schema[:function][:description]

          params = schema[:function][:parameters]
          assert_equal 'object', params[:type]
          assert_equal ['input'], params[:required]
          assert_equal 'string', params[:properties]['input'][:type]
          assert_equal 'Input text', params[:properties]['input'][:description]
        end

        def test_to_anthropic_schema_basic
          @tool.description('Anthropic tool')
          @tool.parameter(:query, type: :string, required: true)

          schema = @tool.to_anthropic_schema

          assert_equal 'test_tool', schema[:name]
          assert_equal 'Anthropic tool', schema[:description]

          input_schema = schema[:input_schema]
          assert_equal 'object', input_schema[:type]
          assert_equal ['query'], input_schema[:required]
          assert_equal 'string', input_schema[:properties]['query'][:type]
        end

        def test_to_json_schema_comprehensive
          @tool.description('JSON Schema tool')
          @tool.parameter(:text, type: :string, required: true)
          @tool.parameter(:count, type: :integer, default: 1)
          @tool.parameter(:enabled, type: :boolean, default: true)

          schema = @tool.to_json_schema

          assert_equal 'http://json-schema.org/draft-07/schema#', schema['$schema']
          assert_equal 'test_tool', schema[:title]
          assert_equal 'JSON Schema tool', schema[:description]
          assert_equal 'object', schema[:type]

          assert_equal ['text'], schema[:required]

          properties = schema[:properties]
          assert_equal 'string', properties['text'][:type]
          assert_equal 'integer', properties['count'][:type]
          assert_equal 1, properties['count'][:default]
          assert_equal 'boolean', properties['enabled'][:type]
          assert_equal true, properties['enabled'][:default]
        end

        # Error handling and validation tests
        def test_call_with_non_hash_parameters
          @tool.execute { |p| p }

          assert_raises(NoMethodError) do
            @tool.call('not a hash')
          end
        end

        def test_call_with_nil_parameters
          @tool.execute { |_p| 'nil params' }

          result = @tool.call(nil)
          assert_equal 'nil params', result
        end

        def test_parameter_with_invalid_enum_type
          assert_raises(ArgumentError) do
            @tool.parameter(:test, enum: 'not an array')
          end
        end

        def test_parameter_with_invalid_properties_type
          assert_raises(ArgumentError) do
            @tool.parameter(:test, properties: 'not a hash')
          end
        end

        # Boundary and performance tests
        def test_large_number_of_parameters
          100.times { |i| @tool.parameter("param#{i}".to_sym, type: :string) }

          assert_equal 100, @tool.parameters.length

          hash = @tool.to_h
          assert_equal 100, hash[:parameters].length
        end

        def test_deeply_nested_parameter_properties
          @tool.parameter(:complex, type: :object, properties: {
                            nested: {
                              deeply: {
                                embedded: 'value'
                              }
                            }
                          })

          param = @tool.parameters.first
          assert_equal :object, param.type
          assert_equal({ nested: { deeply: { embedded: 'value' } } }, param.properties)
        end

        def test_empty_block_initialization
          tool = Definition.new(:empty) {}

          assert_equal :empty, tool.name
          assert_equal '', tool.description
          assert_empty tool.parameters
          assert_nil tool.handler
        end

        def test_block_with_only_comments
          tool = Definition.new(:commented) do
            # This is just a comment
            # No actual DSL calls
          end

          assert_equal :commented, tool.name
          assert_equal '', tool.description
          assert_empty tool.parameters
          assert_nil tool.handler
        end

        # Thread safety and state isolation tests
        def test_multiple_instances_isolation
          tool1 = Definition.new(:tool1) do
            description 'Tool 1'
            parameter :a, type: :string
          end

          tool2 = Definition.new(:tool2) do
            description 'Tool 2'
            parameter :b, type: :number
          end

          assert_equal 'Tool 1', tool1.description
          assert_equal 'Tool 2', tool2.description
          assert_equal :a, tool1.parameters.first.name
          assert_equal :b, tool2.parameters.first.name
        end

        # Schema generation edge cases
        def test_schema_generation_with_special_characters
          tool = Definition.new(:special_chars) do
            description 'Tool with "quotes" and \'apostrophes\''
            parameter :weird_name, type: :string, description: 'Param with <tags> & symbols'
          end

          openai_schema = tool.to_openai_schema
          assert_equal 'special_chars', openai_schema[:function][:name]
          assert_equal 'Tool with "quotes" and \'apostrophes\'', openai_schema[:function][:description]
        end

        def test_schema_generation_with_unicode
          tool = Definition.new(:unicode_tool) do
            description 'Tool with émojis 🌟 and unicode ñoños'
            parameter :café, type: :string, description: 'Café parameter'
          end

          schema = tool.to_json_schema
          assert_equal 'unicode_tool', schema[:title]
          assert_includes schema[:description], '🌟'
          assert schema[:properties].key?('café')
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
