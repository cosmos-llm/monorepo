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

        # --- stop reasons ----------------------------------------------------

        def test_reason_is_finished_when_the_model_stops_calling_tools
          client = FakeClient.new { |_params, _n| FakeResponse.new('done', false, []) }

          result = Loop.run(client: client, tools: [@echo_tool], user: 'hi', max_steps: 5)

          assert_equal :finished, result[:reason]
        end

        def test_reason_is_steps_when_the_cap_is_hit_mid_work
          call = { 'id' => 'call_1', 'name' => 'echo', 'input' => { 'value' => 'x' } }
          client = FakeClient.new { |_params, _n| FakeResponse.new('mid', true, [call]) }

          result = Loop.run(client: client, tools: [@echo_tool], user: 'hi', max_steps: 3)

          assert_equal :steps, result[:reason]
          assert_equal 3, result[:steps_taken]
        end

        def test_session_finish_ends_the_run_without_another_request
          call = { 'id' => 'call_1', 'name' => 'done', 'input' => {} }
          client = FakeClient.new { |_params, _n| FakeResponse.new('', true, [call]) }
          session = Session.new

          done_tool = Definition.new(:done) do
            description 'Finish'
            execute { 'done' }
          end

          result = Loop.run(client: client, tools: [done_tool], user: 'hi', max_steps: 5,
                            session: session) do |_tool_call|
            session.finish!('all set')
            'done'
          end

          assert_equal :finished, result[:reason]
          assert_equal 1, result[:steps_taken]
          # The run ends on the turn the tool finished, not the turn after.
          assert_equal 1, client.requests.length
          assert_equal ['all set'], session.notes
        end

        def test_budget_callback_stops_the_run
          call = { 'id' => 'call_1', 'name' => 'echo', 'input' => { 'value' => 'x' } }
          client = FakeClient.new { |_params, _n| FakeResponse.new('mid', true, [call]) }

          seen = []
          result = Loop.run(client: client, tools: [@echo_tool], user: 'hi', max_steps: 10,
                            budget: lambda { |usage|
                              seen << usage[:steps]
                              usage[:steps] < 2
                            })

          assert_equal :budget, result[:reason]
          assert_equal 2, result[:steps_taken]
          assert_equal [1, 2], seen
        end

        def test_budget_is_not_consulted_when_the_model_stops_on_its_own
          client = FakeClient.new { |_params, _n| FakeResponse.new('done', false, []) }

          called = false
          Loop.run(client: client, tools: [@echo_tool], user: 'hi', max_steps: 5,
                   budget: ->(_usage) { called = true })

          refute called
        end

        # --- usage accounting ------------------------------------------------

        FakeUsage = Struct.new(:prompt_tokens, :completion_tokens, :total_tokens)

        UsageResponse = Struct.new(:text, :tool_use, :raw_tool_calls, :usage) do
          def tool_use?
            tool_use
          end

          def tool_calls
            raw_tool_calls
          end
        end

        def test_usage_accumulates_across_turns
          call = { 'id' => 'call_1', 'name' => 'echo', 'input' => { 'value' => 'x' } }
          responses = [
            UsageResponse.new('', true, [call], FakeUsage.new(10, 5, 15)),
            UsageResponse.new('done', false, [], FakeUsage.new(20, 4, 24))
          ]
          client = FakeClient.new { |_params, n| responses[n - 1] }

          result = Loop.run(client: client, tools: [@echo_tool], user: 'hi', max_steps: 5)

          assert_equal 30, result[:usage][:prompt_tokens]
          assert_equal 9, result[:usage][:completion_tokens]
          assert_equal 39, result[:usage][:total_tokens]
          assert_equal 2, result[:usage][:steps]
        end

        def test_usage_tolerates_a_provider_that_reports_none
          client = FakeClient.new { |_params, _n| FakeResponse.new('done', false, []) }

          result = Loop.run(client: client, tools: [@echo_tool], user: 'hi', max_steps: 5)

          assert_equal 0, result[:usage][:total_tokens]
          assert_equal 1, result[:usage][:steps]
        end

        def test_usage_reads_a_hash_shaped_usage_block
          hash_usage = UsageResponse.new('done', false, [], { 'prompt_tokens' => 7, 'total_tokens' => 9 })
          client = FakeClient.new { |_params, _n| hash_usage }

          result = Loop.run(client: client, tools: [@echo_tool], user: 'hi', max_steps: 5)

          assert_equal 7, result[:usage][:prompt_tokens]
          assert_equal 9, result[:usage][:total_tokens]
        end

        # --- events ----------------------------------------------------------

        def test_on_event_reports_tool_calls_and_final_text
          call = { 'id' => 'call_1', 'name' => 'echo', 'input' => { 'value' => 'hello' } }
          responses = [
            FakeResponse.new('', true, [call]),
            FakeResponse.new('all done', false, [])
          ]
          client = FakeClient.new { |_params, n| responses[n - 1] }

          events = []
          Loop.run(client: client, tools: [@echo_tool], user: 'hi', max_steps: 5,
                   on_event: ->(kind, payload) { events << [kind, payload] })

          assert_equal %i[tool text], events.map(&:first)
          assert_equal :echo, events[0][1][:name]
          assert_equal 'echoed:hello', events[0][1][:result]
          assert_equal 'all done', events[1][1]
        end

        def test_messages_are_returned_for_inspection
          client = FakeClient.new { |_params, _n| FakeResponse.new('done', false, []) }

          result = Loop.run(client: client, tools: [@echo_tool], user: 'hi', max_steps: 5)

          assert_equal [{ role: 'user', content: 'hi' }], result[:messages]
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
