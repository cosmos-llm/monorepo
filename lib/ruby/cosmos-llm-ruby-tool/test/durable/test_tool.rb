# frozen_string_literal: true

require 'test_helper'

module Cosmos
  module Llm
    class TestTool < Minitest::Test
      def setup
        Tool.clear!
      end

      def test_define_simple_tool
        tool = Tool.define(:greet, register: false) do
          description 'Greets a person'
          parameter :name, type: :string, required: true
          execute { |params| "Hello, #{params[:name]}!" }
        end

        assert_equal :greet, tool.name
        assert_equal 'Greets a person', tool.description
        assert_equal 1, tool.parameters.length
      end

      def test_tool_execution
        tool = Tool.define(:add, register: false) do
          description 'Adds two numbers'
          parameter :a, type: :number, required: true
          parameter :b, type: :number, required: true
          execute { |params| params[:a] + params[:b] }
        end

        result = tool.call(a: 5, b: 3)
        assert_equal 8, result
      end

      def test_parameter_validation
        tool = Tool.define(:test, register: false) do
          parameter :required_param, type: :string, required: true
          execute { |params| params[:required_param] }
        end

        error = assert_raises(Tool::ValidationError) do
          tool.call({})
        end

        assert_match(/required/, error.message)
      end

      def test_parameter_with_enum
        tool = Tool.define(:choose, register: false) do
          parameter :option, type: :string, enum: %w[a b c], required: true
          execute { |params| params[:option] }
        end

        assert_equal 'a', tool.call(option: 'a')

        error = assert_raises(Tool::ValidationError) do
          tool.call(option: 'd')
        end

        assert_match(/must be one of/, error.message)
      end

      def test_parameter_with_default
        tool = Tool.define(:greet_default, register: false) do
          parameter :name, type: :string, default: 'World'
          execute { |params| "Hello, #{params[:name]}!" }
        end

        assert_equal 'Hello, World!', tool.call({})
        assert_equal 'Hello, Alice!', tool.call(name: 'Alice')
      end

      def test_registry_operations
        tool1 = Tool.define(:tool1) do
          description 'Tool 1'
        end

        tool2 = Tool.define(:tool2) do
          description 'Tool 2'
        end

        assert_equal 2, Tool.all.length
        assert_equal tool1, Tool.get(:tool1)
        assert_includes Tool.all, tool1
        assert_includes Tool.all, tool2
      end

      def test_openai_schema_generation
        tool = Tool.define(:weather, register: false) do
          description 'Get weather for a location'
          parameter :location, type: :string, required: true
          parameter :units, type: :string, enum: %w[celsius fahrenheit], default: 'celsius'
        end

        schema = tool.to_openai_schema

        assert_equal 'function', schema[:type]
        assert_equal 'weather', schema[:function][:name]
        assert_equal 'Get weather for a location', schema[:function][:description]
        assert_equal 'object', schema[:function][:parameters][:type]
        assert_includes schema[:function][:parameters][:required], 'location'
      end

      def test_anthropic_schema_generation
        tool = Tool.define(:calculate, register: false) do
          description 'Performs calculation'
          parameter :operation, type: :string, required: true
          parameter :a, type: :number, required: true
          parameter :b, type: :number, required: true
        end

        schema = tool.to_anthropic_schema

        assert_equal 'calculate', schema[:name]
        assert_equal 'Performs calculation', schema[:description]
        assert_equal 'object', schema[:input_schema][:type]
        assert_equal 3, schema[:input_schema][:properties].length
        assert_equal 3, schema[:input_schema][:required].length
      end

      def test_json_schema_generation
        tool = Tool.define(:test_schema, register: false) do
          description 'Test schema generation'
          parameter :test_param, type: :string, required: true
        end

        schema = tool.to_json_schema

        assert_equal 'http://json-schema.org/draft-07/schema#', schema[:'$schema']
        assert_equal 'object', schema[:type]
        assert_equal 'test_schema', schema[:title]
        assert_includes schema[:required], 'test_param'
      end

      def test_tool_without_handler
        tool = Tool.define(:no_handler, register: false) do
          description 'Tool without handler'
        end

        error = assert_raises(Tool::NoHandlerError) do
          tool.call({})
        end

        assert_match(/no execution handler/, error.message)
      end

      def test_define_with_register_false
        tool = Tool.define(:not_registered, register: false) do
          description 'Not registered'
        end

        assert_equal :not_registered, tool.name
        assert_nil Tool.get(:not_registered)
      end

      def test_define_with_register_true
        tool = Tool.define(:registered) do
          description 'Registered'
        end

        assert_equal tool, Tool.get(:registered)
        assert_includes Tool.all, tool
      end

      def test_get_non_existent_tool
        assert_nil Tool.get(:non_existent)
      end

      def test_all_tools
        initial_count = Tool.all.length
        tool1 = Tool.define(:all_test1) { description 'Test 1' }
        tool2 = Tool.define(:all_test2) { description 'Test 2' }

        assert_equal initial_count + 2, Tool.all.length
        assert_includes Tool.all, tool1
        assert_includes Tool.all, tool2
      end

      def test_clear_registry
        Tool.define(:clear_test) { description 'To be cleared' }
        refute_nil Tool.get(:clear_test)

        Tool.clear!
        assert_empty Tool.all
        assert_nil Tool.get(:clear_test)
      end

      def test_define_with_nil_name
        assert_raises(ArgumentError) do
          Tool.define(nil) { description 'Nil name' }
        end
      end

      def test_define_with_empty_block
        tool = Tool.define(:empty_block, register: false) {}
        assert_equal :empty_block, tool.name
        assert_equal '', tool.description
        assert_empty tool.parameters
        assert_nil tool.handler
      end

      def test_tool_execution_with_complex_parameters
        tool = Tool.define(:complex_calc, register: false) do
          description 'Complex calculation'
          parameter :a, type: :number, required: true
          parameter :b, type: :number, required: true
          parameter :operation, type: :string, enum: %w[add multiply], default: 'add'
          execute do |params|
            case params[:operation]
            when 'add' then params[:a] + params[:b]
            when 'multiply' then params[:a] * params[:b]
            end
          end
        end

        result = tool.call(a: 3, b: 4, operation: 'multiply')
        assert_equal 12, result

        result = tool.call(a: 3, b: 4) # uses default
        assert_equal 7, result
      end

      def test_schema_generation_with_required_array
        tool = Tool.define(:array_tool, register: false) do
          description 'Tool with array parameter'
          parameter :items, type: :array, required: true
          parameter :count, type: :integer, default: 1
        end

        schema = tool.to_openai_schema
        assert_equal 'function', schema[:type]
        assert_equal 'array_tool', schema[:function][:name]
        assert_includes schema[:function][:parameters][:required], 'items'

        properties = schema[:function][:parameters][:properties]
        assert_equal 'array', properties['items'][:type]
        assert_equal 'integer', properties['count'][:type]
        assert_equal 1, properties['count'][:default]
      end

      def test_registry_isolation
        Tool.clear!
        tool1 = Tool.define(:iso1) { description 'Iso 1' }
        tool2 = Tool.define(:iso2) { description 'Iso 2' }

        assert_equal 2, Tool.all.length
        assert_equal tool1, Tool.get(:iso1)
        assert_equal tool2, Tool.get(:iso2)
      end

      def test_tool_call_with_symbol_keys
        tool = Tool.define(:symbol_keys, register: false) do
          parameter :key1, type: :string
          parameter :key2, type: :number
          execute { |params| "#{params[:key1]}:#{params[:key2]}" }
        end

        result = tool.call(key1: 'test', key2: 42)
        assert_equal 'test:42', result
      end

      def test_tool_call_with_string_keys
        tool = Tool.define(:string_keys, register: false) do
          parameter :key1, type: :string
          parameter :key2, type: :number
          execute { |params| "#{params[:key1]}:#{params[:key2]}" }
        end

        result = tool.call('key1' => 'test', 'key2' => 42)
        assert_equal 'test:42', result
      end

      def test_tool_call_with_mixed_keys
        tool = Tool.define(:mixed_keys, register: false) do
          parameter :key1, type: :string
          parameter :key2, type: :number
          execute { |params| "#{params[:key1]}:#{params[:key2]}" }
        end

        result = tool.call(key1: 'test', 'key2' => 42)
        assert_equal 'test:42', result
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
