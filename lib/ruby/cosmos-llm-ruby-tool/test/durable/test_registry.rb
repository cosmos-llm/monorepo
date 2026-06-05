# frozen_string_literal: true

require 'test_helper'

module Cosmos
  module Llm
    module Tool
      class TestRegistry < Minitest::Test
        def setup
          @registry = Registry.new
          @tool1 = Definition.new(:calculator) do
            description 'Calculator tool'
          end
          @tool2 = Definition.new(:weather) do
            description 'Weather tool'
          end
        end

        def test_initialize
          assert_empty @registry.tools
        end

        def test_register_tool
          result = @registry.register(@tool1)

          assert_equal @tool1, result
          assert_equal 1, @registry.tools.size
          assert_equal @tool1, @registry.tools[:calculator]
        end

        def test_register_multiple_tools
          @registry.register(@tool1)
          @registry.register(@tool2)

          assert_equal 2, @registry.tools.size
          assert_equal @tool1, @registry.tools[:calculator]
          assert_equal @tool2, @registry.tools[:weather]
        end

        def test_register_overwrites_existing
          tool_v1 = Definition.new(:test) { description 'Version 1' }
          tool_v2 = Definition.new(:test) { description 'Version 2' }

          @registry.register(tool_v1)
          @registry.register(tool_v2)

          assert_equal 1, @registry.tools.size
          assert_equal 'Version 2', @registry.get(:test).description
        end

        def test_register_invalid_type
          error = assert_raises(ArgumentError) do
            @registry.register('not a tool')
          end

          assert_match(/Expected a Tool::Definition/, error.message)
        end

        def test_get_existing_tool
          @registry.register(@tool1)

          result = @registry.get(:calculator)

          assert_equal @tool1, result
        end

        def test_get_with_string_name
          @registry.register(@tool1)

          result = @registry.get('calculator')

          assert_equal @tool1, result
        end

        def test_get_nonexistent_tool
          result = @registry.get(:nonexistent)

          assert_nil result
        end

        def test_registered_question_mark_true
          @registry.register(@tool1)

          assert @registry.registered?(:calculator)
        end

        def test_registered_question_mark_false
          refute @registry.registered?(:nonexistent)
        end

        def test_registered_question_mark_with_string
          @registry.register(@tool1)

          assert @registry.registered?('calculator')
        end

        def test_unregister_existing
          @registry.register(@tool1)

          result = @registry.unregister(:calculator)

          assert_equal @tool1, result
          assert_empty @registry.tools
        end

        def test_unregister_nonexistent
          result = @registry.unregister(:nonexistent)

          assert_nil result
        end

        def test_unregister_with_string
          @registry.register(@tool1)

          result = @registry.unregister('calculator')

          assert_equal @tool1, result
        end

        def test_all_empty
          result = @registry.all

          assert_empty result
          assert_instance_of Array, result
        end

        def test_all_with_tools
          @registry.register(@tool1)
          @registry.register(@tool2)

          result = @registry.all

          assert_equal 2, result.length
          assert_includes result, @tool1
          assert_includes result, @tool2
        end

        def test_names_empty
          result = @registry.names

          assert_empty result
          assert_instance_of Array, result
        end

        def test_names_with_tools
          @registry.register(@tool1)
          @registry.register(@tool2)

          result = @registry.names

          assert_equal 2, result.length
          assert_includes result, :calculator
          assert_includes result, :weather
        end

        def test_clear
          @registry.register(@tool1)
          @registry.register(@tool2)

          @registry.clear

          assert_empty @registry.tools
        end

        def test_count_empty
          assert_equal 0, @registry.count
        end

        def test_count_with_tools
          @registry.register(@tool1)
          @registry.register(@tool2)

          assert_equal 2, @registry.count
        end

        def test_size_alias
          @registry.register(@tool1)

          assert_equal 1, @registry.size
          assert_equal @registry.count, @registry.size
        end

        def test_each
          @registry.register(@tool1)
          @registry.register(@tool2)

          tools = []
          @registry.each { |tool| tools << tool }

          assert_equal 2, tools.length
          assert_includes tools, @tool1
          assert_includes tools, @tool2
        end

        def test_each_empty
          tools = []
          @registry.each { |tool| tools << tool }

          assert_empty tools
        end

        def test_to_h_empty
          hash = @registry.to_h

          assert_equal 0, hash[:count]
          assert_empty hash[:tools]
        end

        def test_to_h_with_tools
          @registry.register(@tool1)
          @registry.register(@tool2)

          hash = @registry.to_h

          assert_equal 2, hash[:count]
          assert_equal 2, hash[:tools].length
          assert_includes hash[:tools], :calculator
          assert_includes hash[:tools], :weather
        end

        def test_registry_workflow
          # Register tools
          calc = Definition.new(:calc) { description 'Calc' }
          weather = Definition.new(:weather) { description 'Weather' }

          @registry.register(calc)
          @registry.register(weather)
          assert_equal 2, @registry.count

          # Check tools exist
          assert @registry.registered?(:calc)
          assert @registry.registered?(:weather)

          # Get tools
          assert_equal calc, @registry.get(:calc)
          assert_equal weather, @registry.get(:weather)

          # Iterate
          names = []
          @registry.each { |t| names << t.name }
          assert_equal %i[calc weather].sort, names.sort

          # Unregister one
          @registry.unregister(:calc)
          assert_equal 1, @registry.count
          refute @registry.registered?(:calc)

          # Clear all
          @registry.clear
          assert_equal 0, @registry.count
        end

        def test_each_with_break
          @registry.register(@tool1)
          @registry.register(@tool2)

          tools = []
          @registry.each do |tool|
            tools << tool
            break if tools.size == 1
          end

          assert_equal 1, tools.size
          assert_includes [@tool1, @tool2], tools.first
        end

        def test_register_nil_raises_error
          error = assert_raises(ArgumentError) do
            @registry.register(nil)
          end

          assert_match(/Expected a Tool::Definition/, error.message)
        end

        def test_register_non_definition_raises_error
          error = assert_raises(ArgumentError) do
            @registry.register('string')
          end

          assert_match(/Expected a Tool::Definition/, error.message)
        end

        def test_get_with_nil
          result = @registry.get(nil)

          assert_nil result
        end

        def test_registered_with_nil
          refute @registry.registered?(nil)
        end

        def test_unregister_with_nil
          result = @registry.unregister(nil)

          assert_nil result
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
