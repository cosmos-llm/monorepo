# frozen_string_literal: true

module Cosmos
  module Llm
    module Tool
      # A generic multi-turn ReAct-style tool-calling loop.
      #
      # Calls the model with a tool schema, checks whether it asked to use any
      # tools (+response.tool_use?+), dispatches each requested call against a
      # Registry (always by +tool.name+ — see the Registry docs for why this
      # matters), appends the results as +tool_result+ content blocks, and
      # repeats until the model stops calling tools or +max_steps+ is
      # reached.
      #
      # This is a generalization of the tool loop app-local code has
      # reimplemented more than once (e.g. cosmos-llm-researcher's
      # +CosmosLlmResearcher::Llm#run_tool_loop+); the shape is the same, but
      # this version speaks Cosmos::Llm::Client directly and dispatches
      # through a Cosmos::Llm::Tool::Registry instead of app-local plumbing.
      #
      # It speaks the cosmos-llm provider-neutral response shape
      # (+response.tool_use?+, +response.tool_calls+ ->
      # +[{ 'id', 'name', 'input' }]+, +response.text+) and builds messages in
      # Anthropic content-block format, which the Anthropic provider forwards
      # verbatim. Non-Anthropic providers that don't understand content-block
      # messages or +tool_use?+/+tool_calls+/+text+ are not supported yet.
      #
      # @example Run a tool loop against the Anthropic provider
      #   client = Cosmos::Llm::Client.new(:anthropic, model: 'claude-3-5-sonnet-20240620')
      #   registry = Cosmos::Llm::Tool::Registry.new
      #   registry.register(Cosmos::Llm::Tool::Preset.webfetch)
      #
      #   result = Cosmos::Llm::Tool::Loop.run(
      #     client: client,
      #     tools: registry,
      #     system: 'You are a research assistant.',
      #     user: 'Summarize https://example.com',
      #     max_steps: 10
      #   )
      #   puts result[:text]
      #   puts result[:steps_taken]
      module Loop
        # A tool call surfaced by the model, normalized across providers.
        ToolCall = Struct.new(:id, :name, :input, keyword_init: true)

        # Runs the tool loop to completion.
        #
        # @param client [Cosmos::Llm::Client] a configured cosmos-llm client
        # @param tools [Registry, Array<Definition>] the tools available to the model.
        #   Dispatch is always keyed by +tool.name+ (via Registry#get), never by a
        #   caller-supplied hash key.
        # @param system [String, nil] system prompt
        # @param user [String] initial user message
        # @param max_steps [Integer] hard cap on model turns
        # @param schema [Symbol] which tool schema to send to the provider
        #   (+:anthropic+ or +:openai+)
        # @yield [call] optional block to handle a tool call instead of dispatching
        #   through +tools+. Receives a ToolCall and must return the result content.
        #   Takes priority over registry dispatch when given.
        # @yieldparam call [ToolCall] a normalized tool call to execute
        # @yieldreturn [String] the tool result content
        # @return [Hash] +{ text:, steps_taken: }+ — the model's final text output
        #   and the number of turns actually taken
        # @raise [ToolNotFoundError] if a call names a tool not present in +tools+
        #   and no block is given
        # @example Dispatch through a registry
        #   Loop.run(client: client, tools: registry, system: sys, user: msg, max_steps: 5)
        # @example Dispatch through a custom block
        #   Loop.run(client: client, tools: registry, system: sys, user: msg, max_steps: 5) do |call|
        #     "handled #{call.name}"
        #   end
        def self.run(client:, tools:, user:, system: nil, max_steps: 10, schema: :anthropic, &dispatch)
          registry = as_registry(tools)
          tool_schemas = registry.all.map { |tool| tool_schema(tool, schema) }
          dispatch ||= ->(call) { registry_dispatch(registry, call) }

          messages = [{ role: 'user', content: user }]
          last_text = ''
          steps_taken = 0

          max_steps.times do |i|
            steps_taken = i + 1
            response = client.completion(
              **{ messages: messages, tools: tool_schemas }.tap { |p| p[:system] = system if system }
            )
            last_text = response.text.to_s unless response.text.to_s.empty?

            calls = normalize_tool_calls(response)
            break if calls.empty?

            messages << assistant_message(response, calls)
            messages << tool_results_message(calls, &dispatch)
          end

          { text: last_text, steps_taken: steps_taken }
        end

        # @api private
        def self.as_registry(tools)
          return tools if tools.is_a?(Registry)

          registry = Registry.new
          Array(tools).each { |tool| registry.register(tool) }
          registry
        end
        private_class_method :as_registry

        # @api private
        def self.tool_schema(tool, schema)
          case schema
          when :openai then tool.to_openai_schema
          else tool.to_anthropic_schema
          end
        end
        private_class_method :tool_schema

        # @api private
        def self.registry_dispatch(registry, call)
          tool = registry.get(call.name)
          raise ToolNotFoundError, "No tool registered for '#{call.name}'" unless tool

          tool.call(call.input).to_s
        end
        private_class_method :registry_dispatch

        # @api private
        def self.normalize_tool_calls(response)
          return [] unless response.respond_to?(:tool_use?) && response.tool_use?
          return [] unless response.respond_to?(:tool_calls)

          Array(response.tool_calls).map do |call|
            ToolCall.new(
              id: call['id'] || call[:id],
              name: (call['name'] || call[:name]).to_s.to_sym,
              input: call['input'] || call[:input] || {}
            )
          end
        end
        private_class_method :normalize_tool_calls

        # Reconstruct the assistant turn as Anthropic content blocks: any text
        # the model produced, followed by one tool_use block per call. The
        # provider forwards this content verbatim, so it must round-trip the
        # tool_use ids.
        # @api private
        def self.assistant_message(response, calls)
          blocks = []
          text = response.text.to_s
          blocks << { 'type' => 'text', 'text' => text } unless text.empty?
          calls.each do |call|
            blocks << {
              'type' => 'tool_use',
              'id' => call.id,
              'name' => call.name.to_s,
              'input' => call.input
            }
          end
          { role: 'assistant', content: blocks }
        end
        private_class_method :assistant_message

        # @api private
        def self.tool_results_message(calls, &dispatch)
          blocks = calls.map do |call|
            result = begin
              dispatch.call(call)
            rescue ToolNotFoundError
              raise
            rescue StandardError => e
              "Tool error: #{e.message}"
            end
            {
              'type' => 'tool_result',
              'tool_use_id' => call.id,
              'content' => result.to_s
            }
          end
          { role: 'user', content: blocks }
        end
        private_class_method :tool_results_message
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
