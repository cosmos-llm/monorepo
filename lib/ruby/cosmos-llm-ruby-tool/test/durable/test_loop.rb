# frozen_string_literal: true

require 'test_helper'

module Cosmos
  module Llm
    module Tool
      class TestLoop < Minitest::Test
        FakeResponse = Struct.new(:text, :tool_use, :raw_tool_calls) do
          def tool_use?
            tool_use
          end

          def tool_calls
            raw_tool_calls
          end
        end

        class FakeClient
          attr_reader :requests

          def initialize(&responder)
            @requests = []
            @responder = responder
          end

          def completion(params)
            @requests << params
            @responder.call(params, @requests.length)
          end
        end

        def setup
          @echo_tool = Definition.new(:echo) do
            description 'Echoes input'
            parameter :value, type: :string, required: true
            execute { |params| "echoed:#{params[:value]}" }
          end
        end

        def test_returns_immediately_when_no_tool_use
          client = FakeClient.new { |_params, _n| FakeResponse.new('final answer', false, []) }

          result = Loop.run(client: client, tools: [@echo_tool], user: 'hi', max_steps: 5)

          assert_equal 'final answer', result[:text]
          assert_equal 1, result[:steps_taken]
          assert_equal 1, client.requests.length
        end

        def test_dispatches_tool_calls_through_registry_by_name
          responses = [
            FakeResponse.new('', true, [{ 'id' => 'call_1', 'name' => 'echo', 'input' => { 'value' => 'hello' } }]),
            FakeResponse.new('done', false, [])
          ]
          client = FakeClient.new { |_params, n| responses[n - 1] }

          result = Loop.run(client: client, tools: [@echo_tool], user: 'hi', max_steps: 5)

          assert_equal 'done', result[:text]
          assert_equal 2, result[:steps_taken]

          # Second request should carry the tool_result with the dispatched value
          second_request = client.requests[1]
          tool_result_block = second_request[:messages].last[:content].first
          assert_equal 'tool_result', tool_result_block['type']
          assert_equal 'call_1', tool_result_block['tool_use_id']
          assert_equal 'echoed:hello', tool_result_block['content']
        end

        def test_accepts_a_registry_directly
          registry = Registry.new
          registry.register(@echo_tool)

          responses = [
            FakeResponse.new('', true, [{ 'id' => 'call_1', 'name' => 'echo', 'input' => { 'value' => 'x' } }]),
            FakeResponse.new('done', false, [])
          ]
          client = FakeClient.new { |_params, n| responses[n - 1] }

          result = Loop.run(client: client, tools: registry, user: 'hi', max_steps: 5)

          assert_equal 'done', result[:text]
        end

        def test_stops_at_max_steps_even_if_model_keeps_calling_tools
          call = { 'id' => 'call_1', 'name' => 'echo', 'input' => { 'value' => 'x' } }
          client = FakeClient.new { |_params, _n| FakeResponse.new('mid', true, [call]) }

          result = Loop.run(client: client, tools: [@echo_tool], user: 'hi', max_steps: 3)

          assert_equal 3, result[:steps_taken]
          assert_equal 3, client.requests.length
        end

        def test_unregistered_tool_call_raises_tool_not_found
          call = { 'id' => 'call_1', 'name' => 'nonexistent', 'input' => {} }
          client = FakeClient.new { |_params, n| n == 1 ? FakeResponse.new('', true, [call]) : FakeResponse.new('done', false, []) }

          assert_raises(ToolNotFoundError) do
            Loop.run(client: client, tools: [@echo_tool], user: 'hi', max_steps: 5)
          end
        end

        def test_custom_dispatch_block_takes_priority_over_registry
          call = { 'id' => 'call_1', 'name' => 'echo', 'input' => { 'value' => 'x' } }
          responses = [
            FakeResponse.new('', true, [call]),
            FakeResponse.new('done', false, [])
          ]
          client = FakeClient.new { |_params, n| responses[n - 1] }

          seen = []
          result = Loop.run(client: client, tools: [@echo_tool], user: 'hi', max_steps: 5) do |tool_call|
            seen << tool_call.name
            'custom result'
          end

          assert_equal [:echo], seen
          second_request = client.requests[1]
          tool_result_block = second_request[:messages].last[:content].first
          assert_equal 'custom result', tool_result_block['content']
          assert_equal 'done', result[:text]
        end

        def test_dispatch_error_is_captured_as_tool_error_content
          call = { 'id' => 'call_1', 'name' => 'echo', 'input' => { 'value' => 'x' } }
          responses = [
            FakeResponse.new('', true, [call]),
            FakeResponse.new('done', false, [])
          ]
          client = FakeClient.new { |_params, n| responses[n - 1] }

          Loop.run(client: client, tools: [@echo_tool], user: 'hi', max_steps: 5) do |_tool_call|
            raise StandardError, 'boom'
          end

          second_request = client.requests[1]
          tool_result_block = second_request[:messages].last[:content].first
          assert_match(/Tool error: boom/, tool_result_block['content'])
        end

        def test_sends_system_prompt_when_given
          client = FakeClient.new { |_params, _n| FakeResponse.new('ok', false, []) }

          Loop.run(client: client, tools: [@echo_tool], user: 'hi', system: 'be nice', max_steps: 5)

          assert_equal 'be nice', client.requests.first[:system]
        end

        def test_omits_system_key_when_not_given
          client = FakeClient.new { |_params, _n| FakeResponse.new('ok', false, []) }

          Loop.run(client: client, tools: [@echo_tool], user: 'hi', max_steps: 5)

          refute client.requests.first.key?(:system)
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
