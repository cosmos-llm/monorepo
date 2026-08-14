# frozen_string_literal: true

require 'cosmos/llm/signature'

require_relative 'module'
require_relative 'prediction'
require_relative 'settings'

module Cosmos
  module Llm
    module Predict
      # One LLM call, described by a signature.
      #
      # This is where the pieces meet: the adapter renders the signature and
      # inputs into messages, the client sends them, and the adapter parses the
      # reply back into typed values. Everything else in this gem either builds
      # on Predict or feeds it.
      #
      # @example Direct use
      #   predictor = Predict.new('question -> answer')
      #   prediction = predictor.call(question: 'What is 2+2?')
      #   prediction.answer  # => '4'
      #
      # @example With a declared signature and few-shot demos
      #   predictor = Predict.new(Summarize)
      #   predictor.demos = [Example.new(document: '...', summary: '...')]
      #   predictor.call(document: text)
      class Predict < Module
        # @return [Class] the signature this predictor satisfies
        attr_reader :signature

        # @return [Array<Example>] few-shot demonstrations included in the prompt
        attr_accessor :demos

        # Builds a predictor.
        #
        # @param signature [Class, String] a signature class or shorthand string
        # @param client [Object, nil] the LLM client; defaults to the configured one
        # @param adapter [Adapters::Base, nil] the adapter; defaults to the configured one
        # @param cache [Cache, nil] a cache for completions; defaults to the configured one
        # @param demos [Array<Example>] few-shot demonstrations
        # @param params [Hash] extra parameters merged into every request
        # @raise [Cosmos::Llm::Signature::DefinitionError] if the signature is invalid
        # @example
        #   Predict.new('question -> answer', temperature: 0.0)
        def initialize(signature, client: nil, adapter: nil, cache: nil, demos: [], **params)
          @signature = Cosmos::Llm::Signature.coerce(signature)
          @client = client
          @adapter = adapter
          @cache = cache
          @demos = demos
          @params = params
        end

        # @return [Object] the client in use, falling back to settings
        # @raise [ConfigurationError] if no client is configured
        def client
          resolved = @client || Predict.settings_client
          raise ConfigurationError, no_client_message unless resolved

          resolved
        end

        # @return [Adapters::Base] the adapter in use, falling back to settings
        def adapter
          @adapter || Cosmos::Llm::Predict.settings.adapter
        end

        # @return [Cache, nil] the cache in use, falling back to settings
        def cache
          @cache || Cosmos::Llm::Predict.settings.cache
        end

        # Runs the call.
        #
        # On a parse failure the call is retried, with the model's malformed
        # reply and the parse error fed back as a correction turn. Models
        # usually fix a format mistake when shown it, and this costs one extra
        # call rather than failing the whole run.
        #
        # @param inputs [Hash] the input field values
        # @return [Prediction] the typed result
        # @raise [ParseError] if the output cannot be parsed after retries
        # @raise [ConfigurationError] if no client is configured
        def forward(**inputs)
          coerced = @signature.coerce_inputs(inputs)
          messages = adapter.format(@signature, coerced, @demos)
          attempts = Cosmos::Llm::Predict.settings.max_parse_retries + 1
          last_error = nil

          attempts.times do |attempt|
            completion, usage, cached = complete(messages)
            begin
              values = adapter.parse(@signature, completion)
              return build_prediction(values, completion, usage, cached)
            rescue ParseError => e
              last_error = e
              messages = messages + correction_turn(completion, e) if attempt < attempts - 1
            end
          end

          raise last_error
        end

        # Returns a copy of this predictor with different demonstrations.
        #
        # @param demos [Array<Example>] the new demonstrations
        # @return [Predict] a new predictor; the receiver is unchanged
        def with_demos(demos)
          copy = dup
          copy.demos = demos
          copy
        end

        # Returns a copy of this predictor with different instructions.
        #
        # @param text [String] the new instructions
        # @return [Predict] a new predictor; the receiver is unchanged
        def with_instructions(text)
          copy = dup
          copy.instance_variable_set(:@signature, @signature.with_instructions(text))
          copy
        end

        # A deep copy sharing no mutable state with the receiver.
        #
        # @return [Predict] an independent copy
        def deep_copy
          copy = dup
          copy.demos = @demos.dup
          copy
        end

        # The tunable state: instructions and demonstrations.
        #
        # @return [Hash] serializable predictor state
        def dump_state
          {
            'instructions' => @signature.instructions,
            'demos' => @demos.map { |demo| demo.respond_to?(:to_h) ? demo.to_h : demo }
          }
        end

        # Restores state produced by {#dump_state}.
        #
        # @param state [Hash] the state to restore
        # @return [self]
        def load_state(state)
          instructions = state['instructions'] || state[:instructions]
          @signature = @signature.with_instructions(instructions) if instructions

          demos = state['demos'] || state[:demos]
          @demos = Array(demos).map { |demo| demo.is_a?(Example) ? demo : Example.new(**symbolize(demo)) } if demos

          self
        end

        # @return [String] string representation
        def to_s
          "#<Predict #{@signature}>"
        end
        alias inspect to_s

        # @return [Object, nil] the configured default client
        def self.settings_client
          Cosmos::Llm::Predict.settings.client
        end

        private

        # Sends the messages, going through the cache when one is configured.
        #
        # @param messages [Array<Hash>] the messages to send
        # @return [Array(String, Hash, Boolean)] completion text, usage, cache hit
        def complete(messages)
          request = request_for(messages)
          store = cache
          return [*call_client(request), false] unless store

          key = Cache.key_for(
            signature: @signature.cache_signature,
            adapter: adapter.class.name,
            request: cacheable(request)
          )
          hit = store.read(key)
          return [hit[:value]['completion'], hit[:value]['usage'], true] if hit

          completion, usage = call_client(request)
          store.write(key, { 'completion' => completion, 'usage' => usage })
          [completion, usage, false]
        end

        # @param request [Hash] the completion parameters
        # @return [Array(String, Hash)] completion text and usage
        def call_client(request)
          response = client.completion(**request)
          [text_of(response), usage_of(response)]
        end

        # Builds the completion request parameters.
        #
        # @param messages [Array<Hash>] the messages to send
        # @return [Hash] parameters for the client
        def request_for(messages)
          settings = Cosmos::Llm::Predict.settings
          request = { messages: messages, max_tokens: settings.max_tokens }
          request[:model] = settings.model if settings.model
          request[:temperature] = settings.temperature unless settings.temperature.nil?
          request.merge!(adapter.request_params(@signature))
          request.merge!(@params)
          request
        end

        # Strips non-serializable values so the request can key a cache entry.
        #
        # @param request [Hash] the completion parameters
        # @return [Hash] a JSON-serializable view of the request
        def cacheable(request)
          request.reject { |_key, value| value.respond_to?(:call) }
        end

        # Extracts assistant text from a provider response.
        #
        # Prefers the provider-neutral +text+ method, falling back to
        # +ResponseHelpers+ for providers or doubles that only expose +choices+.
        #
        # @param response [Object] the provider response
        # @return [String] the assistant text
        # @raise [ConfigurationError] if the response exposes neither shape
        def text_of(response)
          return response.text.to_s if response.respond_to?(:text)

          content = content_from_choices(response)
          return content.to_s unless content.nil?

          raise ConfigurationError,
                "Cannot read text from a #{response.class} response. Expected #text (the provider-neutral " \
                'shape) or #choices -> #message -> #content.'
        end

        # Digs assistant text out of a choices/message/content response.
        #
        # Older provider responses and hand-rolled doubles expose this shape
        # instead of +#text+. Read it directly rather than through
        # +ResponseHelpers+, so this works whether or not the client gem happens
        # to be loaded.
        #
        # @param response [Object] the provider response
        # @return [String, nil] the text, or nil if the response is another shape
        def content_from_choices(response)
          return nil unless response.respond_to?(:choices)

          choice = Array(response.choices).first
          return nil unless choice.respond_to?(:message)

          message = choice.message
          message.respond_to?(:content) ? message.content : nil
        end

        # @param response [Object] the provider response
        # @return [Hash, nil] token usage, if the provider reported any
        def usage_of(response)
          response.respond_to?(:usage) ? response.usage : nil
        end

        # Builds the correction turn appended after a parse failure.
        #
        # @param completion [String] the malformed reply
        # @param error [ParseError] the parse failure
        # @return [Array<Hash>] the assistant reply and the correction request
        def correction_turn(completion, error)
          [
            { role: 'assistant', content: completion },
            { role: 'user',
              content: "That response could not be parsed: #{error.message}\n\n" \
                       'Reply again with the same content in the required format. Output only the formatted fields.' }
          ]
        end

        # @param values [Hash] the coerced output values
        # @param completion [String] the raw model output
        # @param usage [Hash, nil] token usage
        # @param cached [Boolean] whether the completion came from the cache
        # @return [Prediction] the assembled prediction
        def build_prediction(values, completion, usage, cached)
          prediction = Prediction.new(**values)
          prediction.completion = completion
          prediction.usage = usage
          prediction.signature = @signature
          prediction.cached = cached
          prediction
        end

        # @param hash [Hash] a hash with string or symbol keys
        # @return [Hash{Symbol => Object}] the same hash with symbol keys
        def symbolize(hash)
          hash.each_with_object({}) { |(key, value), result| result[key.to_s.to_sym] = value }
        end

        # @return [String] guidance shown when no client is configured
        def no_client_message
          "No LLM client configured. Pass one to Predict.new(client: ...) or set a default:\n" \
            "  Cosmos::Llm::Predict.configure do |settings|\n" \
            "    settings.client = Cosmos::Llm::Client.new(:anthropic, model: 'claude-opus-4')\n" \
            '  end'
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
