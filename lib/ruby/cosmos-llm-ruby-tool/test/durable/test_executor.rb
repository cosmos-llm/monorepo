# frozen_string_literal: true

require 'test_helper'

module Cosmos
  module Llm
    module Tool
      class TestExecutor < Minitest::Test
        def setup
          @tool = Definition.new(:test) do
            parameter :a, type: :number, required: true
            parameter :b, type: :number, required: true
            execute { |params| params[:a] + params[:b] }
          end
        end

        def test_execute_success
          result = Executor.execute(@tool, { a: 5, b: 3 })

          assert_equal 8, result
        end

        def test_execute_with_string_keys
          result = Executor.execute(@tool, { 'a' => 5, 'b' => 3 })

          assert_equal 8, result
        end

        def test_execute_no_handler
          tool = Definition.new(:no_handler) do
            parameter :x, type: :number
          end

          error = assert_raises(NoHandlerError) do
            Executor.execute(tool, { x: 5 })
          end

          assert_match(/no execution handler/, error.message)
          assert_match(/no_handler/, error.message)
        end

        def test_execute_validation_failure
          error = assert_raises(ValidationError) do
            Executor.execute(@tool, { a: 5 }) # Missing required param :b
          end

          assert_match(/required/, error.message)
        end

        def test_execute_with_default_values
          tool = Definition.new(:with_defaults) do
            parameter :name, type: :string, default: 'World'
            parameter :greeting, type: :string, default: 'Hello'
            execute { |params| "#{params[:greeting]}, #{params[:name]}!" }
          end

          result = Executor.execute(tool, {})

          assert_equal 'Hello, World!', result
        end

        def test_execute_default_override
          tool = Definition.new(:with_defaults) do
            parameter :value, type: :number, default: 10
            execute { |params| params[:value] * 2 }
          end

          result = Executor.execute(tool, { value: 5 })

          assert_equal 10, result # Should use provided value, not default
        end

        def test_execute_handler_exception
          tool = Definition.new(:error_tool) do
            parameter :x, type: :number
            execute { |_params| raise StandardError, 'Handler error' }
          end

          error = assert_raises(ExecutionError) do
            Executor.execute(tool, { x: 1 })
          end

          assert_match(/Error executing tool/, error.message)
          assert_match(/error_tool/, error.message)
          assert_match(/Handler error/, error.message)
        end

        def test_normalize_params_symbol_keys
          params = { a: 1, b: 2 }

          result = Executor.normalize_params(params)

          assert_equal({ a: 1, b: 2 }, result)
        end

        def test_normalize_params_string_keys
          params = { 'a' => 1, 'b' => 2 }

          result = Executor.normalize_params(params)

          assert_equal({ a: 1, b: 2 }, result)
        end

        def test_normalize_params_mixed_keys
          params = { 'a' => 1, b: 2, 'c' => 3 }

          result = Executor.normalize_params(params)

          assert_equal({ a: 1, b: 2, c: 3 }, result)
        end

        def test_validate_and_apply_defaults_all_provided
          tool = Definition.new(:test) do
            parameter :a, type: :number, required: true
            parameter :b, type: :number, default: 10
          end

          result = Executor.validate_and_apply_defaults(tool, { a: 5, b: 3 })

          assert_equal({ a: 5, b: 3 }, result)
        end

        def test_validate_and_apply_defaults_uses_default
          tool = Definition.new(:test) do
            parameter :a, type: :number, required: true
            parameter :b, type: :number, default: 10
          end

          result = Executor.validate_and_apply_defaults(tool, { a: 5 })

          assert_equal({ a: 5, b: 10 }, result)
        end

        def test_validate_and_apply_defaults_omits_nil
          tool = Definition.new(:test) do
            parameter :a, type: :number, required: true
            parameter :b, type: :number, required: false
          end

          result = Executor.validate_and_apply_defaults(tool, { a: 5 })

          assert_equal({ a: 5 }, result)
          refute result.key?(:b)
        end

        def test_validate_and_apply_defaults_validation_error
          tool = Definition.new(:test) do
            parameter :a, type: :number, required: true
          end

          error = assert_raises(ValidationError) do
            Executor.validate_and_apply_defaults(tool, {})
          end

          assert_match(/required/, error.message)
        end

        def test_execute_complex_workflow
          tool = Definition.new(:calculator) do
            parameter :operation, type: :string, enum: %w[add subtract multiply divide], required: true
            parameter :a, type: :number, required: true
            parameter :b, type: :number, required: true
            parameter :round, type: :boolean, default: false

            execute do |params|
              result = case params[:operation]
                       when 'add' then params[:a] + params[:b]
                       when 'subtract' then params[:a] - params[:b]
                       when 'multiply' then params[:a] * params[:b]
                       when 'divide' then params[:a].to_f / params[:b]
                       end

              params[:round] ? result.round : result
            end
          end

          # Test addition
          assert_equal 8, Executor.execute(tool, { operation: 'add', a: 5, b: 3 })

          # Test division without rounding
          assert_equal 2.5, Executor.execute(tool, { operation: 'divide', a: 5, b: 2 })

          # Test division with rounding
          assert_equal 3, Executor.execute(tool, { operation: 'divide', a: 5, b: 2, round: true })

          # Test enum validation
          error = assert_raises(ValidationError) do
            Executor.execute(tool, { operation: 'invalid', a: 5, b: 3 })
          end
          assert_match(/must be one of/, error.message)
        end

        def test_execute_with_optional_parameters
          tool = Definition.new(:greeter) do
            parameter :name, type: :string, required: true
            parameter :title, type: :string, required: false
            parameter :formal, type: :boolean, default: false

            execute do |params|
              greeting = params[:formal] ? 'Good day' : 'Hello'
              name_part = params[:title] ? "#{params[:title]} #{params[:name]}" : params[:name]
              "#{greeting}, #{name_part}!"
            end
          end

          # Minimal params
          assert_equal 'Hello, Alice!', Executor.execute(tool, { name: 'Alice' })

          # With optional title
          assert_equal 'Hello, Dr. Smith!', Executor.execute(tool, { name: 'Smith', title: 'Dr.' })

          # Formal mode
          assert_equal 'Good day, Alice!', Executor.execute(tool, { name: 'Alice', formal: true })

          # All parameters
          assert_equal 'Good day, Prof. Johnson!',
                       Executor.execute(tool, { name: 'Johnson', title: 'Prof.', formal: true })
        end

        def test_execute_with_nil_params
          tool = Definition.new(:test) do
            parameter :a, type: :number, required: true
            execute { |params| params[:a] }
          end

          error = assert_raises(ValidationError) do
            Executor.execute(tool, nil)
          end

          assert_match(/required/, error.message)
        end

        def test_execute_with_empty_params_hash
          tool = Definition.new(:test) do
            parameter :a, type: :number, default: 42
            execute { |params| params[:a] }
          end

          result = Executor.execute(tool, {})

          assert_equal 42, result
        end

        def test_execute_with_extra_params
          tool = Definition.new(:test) do
            parameter :a, type: :number, required: true
            execute { |params| params[:a] }
          end

          # Extra params should be ignored during validation
          result = Executor.execute(tool, { a: 5, extra: 'ignored' })

          assert_equal 5, result
        end

        def test_execute_handler_returns_nil
          tool = Definition.new(:nil_returner) do
            parameter :x, type: :number
            execute { |_params| nil }
          end

          result = Executor.execute(tool, { x: 1 })

          assert_nil result
        end

        def test_execute_handler_returns_complex_object
          tool = Definition.new(:object_returner) do
            parameter :data, type: :object
            execute { |params| { result: params[:data], processed: true } }
          end

          input = { key: 'value', number: 42 }
          result = Executor.execute(tool, { data: input })

          expected = { result: input, processed: true }
          assert_equal expected, result
        end

        def test_execute_handler_returns_array
          tool = Definition.new(:array_returner) do
            parameter :items, type: :array
            execute { |params| params[:items].map(&:upcase) }
          end

          result = Executor.execute(tool, { items: %w[a b c] })

          assert_equal %w[A B C], result
        end

        def test_normalize_params_with_nested_structures
          # Test that nested hashes and arrays are not modified
          params = {
            'config' => { 'host' => 'localhost', 'port' => 8080 },
            'items' => %w[a b c]
          }

          result = Executor.normalize_params(params)

          expected = {
            config: { 'host' => 'localhost', 'port' => 8080 },
            items: %w[a b c]
          }
          assert_equal expected, result
        end

        def test_validate_and_apply_defaults_multiple_required_missing
          tool = Definition.new(:test) do
            parameter :a, type: :number, required: true
            parameter :b, type: :string, required: true
            parameter :c, type: :boolean, required: true
          end

          error = assert_raises(ValidationError) do
            Executor.validate_and_apply_defaults(tool, {})
          end

          # Should mention all missing required parameters
          assert_match(/required/, error.message)
        end

        def test_validate_and_apply_defaults_type_mismatch_with_enum
          tool = Definition.new(:test) do
            parameter :choice, type: :string, enum: %w[red blue green], required: true
          end

          error = assert_raises(ValidationError) do
            Executor.validate_and_apply_defaults(tool, { choice: 'yellow' })
          end

          assert_match(/must be one of/, error.message)
          assert_match(/red/, error.message)
          assert_match(/blue/, error.message)
          assert_match(/green/, error.message)
        end

        def test_execute_preserves_parameter_order
          tool = Definition.new(:ordered) do
            parameter :first, type: :string, required: true
            parameter :second, type: :string, required: true
            parameter :third, type: :string, default: 'default'
            execute { |params| [params[:first], params[:second], params[:third]] }
          end

          result = Executor.execute(tool, { first: 'a', second: 'b', third: 'c' })

          assert_equal %w[a b c], result
        end

        def test_execute_with_false_boolean_default
          tool = Definition.new(:test) do
            parameter :enabled, type: :boolean, default: false
            execute { |params| params[:enabled] ? 'on' : 'off' }
          end

          result = Executor.execute(tool, {})

          assert_equal 'off', result
        end

        def test_execute_with_zero_number_default
          tool = Definition.new(:test) do
            parameter :count, type: :number, default: 0
            execute { |params| "Count: #{params[:count]}" }
          end

          result = Executor.execute(tool, {})

          assert_equal 'Count: 0', result
        end

        def test_execute_with_empty_string_default
          tool = Definition.new(:test) do
            parameter :message, type: :string, default: ''
            execute { |params| "Message: '#{params[:message]}'" }
          end

          result = Executor.execute(tool, {})

          assert_equal "Message: ''", result
        end

        def test_execute_with_empty_array_default
          tool = Definition.new(:test) do
            parameter :items, type: :array, default: []
            execute { |params| "Items: #{params[:items].join(', ')}" }
          end

          result = Executor.execute(tool, {})

          assert_equal 'Items: ', result
        end

        def test_execute_with_empty_hash_default
          tool = Definition.new(:test) do
            parameter :config, type: :object, default: {}
            execute { |params| "Keys: #{params[:config].keys.join(', ')}" }
          end

          result = Executor.execute(tool, {})

          assert_equal 'Keys: ', result
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
