# frozen_string_literal: true

require 'minitest/autorun'
require 'mocha/minitest'

# Add lib to load path
$LOAD_PATH.unshift File.expand_path('../lib', __dir__)

# Mock the cosmos-llm-tool dependency before requiring anything
module Cosmos
  module Llm
    module Tool
      def self.define(name, register: false, &block) # rubocop:disable Lint/UnusedMethodArgument
        # Create a mock tool definition that can execute the block
        MockToolDefinition.new(name, &block)
      end

      class MockToolDefinition
        def initialize(name, &block)
          @name = name
          @block = block
          @parameters = []
        end

        def description(desc)
          @description = desc
        end

        def parameter(name, **options)
          @parameters << { name: name, **options }
        end

        def call(params = {})
          # Execute the block with the parameters
          instance_exec(params, &@block)
        end

        def to_openai_schema
          {
            'function' => {
              'name' => @name.to_s,
              'description' => @description,
              'parameters' => {
                'type' => 'object',
                'properties' => @parameters.each_with_object({}) do |param, hash|
                  hash[param[:name].to_s] = {
                    'type' => param[:type].to_s,
                    'description' => param[:description]
                  }
                  hash[param[:name].to_s]['required'] = param[:required] if param.key?(:required)
                end,
                'required' => @parameters.select { |p| p[:required] }.map { |p| p[:name].to_s }
              }
            }
          }
        end

        def to_anthropic_schema
          # Mock anthropic schema
          { 'name' => @name.to_s, 'description' => @description }
        end
      end
    end
  end
end

# Mock the require to avoid loading the actual gem
$LOADED_FEATURES << 'cosmos/llm/tool.rb'

require 'cosmos/llm/tool/preset'
