# frozen_string_literal: true

# OpenRouter provider for accessing various language models through the OpenRouter API.

require 'cosmos/llm/http_client'
require 'json'
require 'cosmos/llm/errors'
require 'cosmos/llm/providers/base'
require 'cosmos/llm/providers/openrouter_routing'

module Cosmos
  module Llm
    module Providers
      # OpenRouter provider for accessing various language models through the OpenRouter API.
      # Provides completion, embedding, and streaming capabilities with authentication handling,
      # error management, and response normalization.
      class OpenRouter < Cosmos::Llm::Providers::Base
        BASE_URL = 'https://openrouter.ai/api/v1'

        def default_api_key
          begin
            Cosmos::Llm.configuration.openrouter&.api_key
          rescue NoMethodError
            nil
          end || ENV['OPENROUTER_API_KEY']
        end

        attr_accessor :api_key

        # @param api_key [String, nil] OpenRouter API key
        # @param routing [Routing, Hash, nil] provider-routing preference applied
        #   to every request. A request carrying its own `provider` block
        #   replaces this wholesale rather than merging field by field: merging
        #   would let a client-wide `only` leak into a request that asked to
        #   route purely by price, which is the opposite of what a per-request
        #   preference should mean.
        # @param report_route [Boolean] ask OpenRouter which upstream served each
        #   request. Off by default because it enlarges every response for
        #   callers who never read it. Worth enabling whenever a routing
        #   preference is set: it is the only way to tell an `order` that was
        #   honoured from one that quietly fell through to a fallback.
        def initialize(api_key: nil, routing: nil, report_route: false)
          super()
          @api_key = api_key || default_api_key
          @routing = normalize_routing(routing)
          @report_route = report_route
          @conn = Cosmos::Llm::HttpClient.new(url: BASE_URL)
        end

        # @return [Routing, nil] the routing preference applied to every request
        attr_reader :routing

        # @return [Boolean] whether responses carry the serving provider
        attr_reader :report_route

        def completion(options)
          options = apply_routing(options)

          response = @conn.post('chat/completions') do |req|
            req.headers['Authorization'] = "Bearer #{@api_key}"
            req.headers['X-OpenRouter-Metadata'] = 'enabled' if @report_route
            req.body = options
          end

          handle_response(response)
        end

        def embedding(model:, input:, **options)
          response = @conn.post('embeddings') do |req|
            req.headers['Authorization'] = "Bearer #{@api_key}"
            req.body = { model: model, input: input, **options }
          end

          handle_response(response, OpenRouterEmbeddingResponse)
        end

        def models
          response = @conn.get('models') do |req|
            req.headers['Authorization'] = "Bearer #{@api_key}"
          end

          handle_response(response).data.map { |model| model['id'] }
        end

        def self.stream?
          true
        end
        def stream(options, &block)
          options = apply_routing(options)
          options[:stream] = true
          options['temperature'] = options['temperature'].to_f if options['temperature']

          response = @conn.post_stream('chat/completions') do |stream|
            stream.on_chunk { |chunk| block.call(OpenRouterStreamResponse.new(chunk)) }
            stream.headers['Authorization'] = "Bearer #{@api_key}"
            stream.headers['Accept'] = 'text/event-stream'
            stream.headers['X-OpenRouter-Metadata'] = 'enabled' if @report_route
            stream.body = options
          end

          handle_response(response)
        end

        private

        # Coerces a Routing, a Hash, or nil into a Routing (or nil).
        def normalize_routing(routing)
          case routing
          when nil then nil
          when Routing then routing
          when Hash then Routing.from_h(routing)
          else
            raise ArgumentError,
                  "routing must be a #{Routing} or a Hash, got #{routing.class}."
          end
        end

        # Applies the client-wide routing default, leaving a per-request
        # `provider` block untouched where the caller set one.
        def apply_routing(options)
          return options if @routing.nil? || @routing.empty?
          return options if options.key?(:provider) || options.key?('provider')

          options.merge(provider: @routing.to_h)
        end

        def handle_response(response, response_class = OpenRouterResponse)
          case response.status
          when 200..299
            response_class.new(response.body)
          when 401
            raise Cosmos::Llm::AuthenticationError, parse_error_message(response)
          when 429
            raise Cosmos::Llm::RateLimitError, parse_error_message(response)
          when 400..499
            raise Cosmos::Llm::InvalidRequestError, parse_error_message(response)
          when 500..599
            raise Cosmos::Llm::ServerError, parse_error_message(response)
          else
            raise Cosmos::Llm::APIError, "Unexpected response code: #{response.status}"
          end
        end

        def parse_error_message(response)
          body = begin
            JSON.parse(response.body)
          rescue StandardError
            nil
          end
          message = body&.dig('error', 'message') || response.body
          "#{response.status} Error: #{message}"
        end

        # Response wrapper for OpenRouter API completion responses.
        class OpenRouterResponse
          attr_reader :raw_response

          def initialize(response)
            @raw_response = response
          end

          def choices
            @raw_response['choices'].map { |choice| OpenRouterChoice.new(choice) }
          end

          def data
            @raw_response['data']
          end

          # The upstream provider that actually served this request.
          #
          # Populated only when the provider was built with
          # `report_route: true`, and absent on a cache hit even then, so nil
          # means "not stated" rather than "not routed".
          #
          # This is what makes a routing preference verifiable: without it, an
          # `order` that silently fell through to a fallback looks exactly like
          # one that was honoured.
          #
          # @return [String, nil] e.g. "Anthropic"
          def served_by
            selected_endpoint&.dig('provider')
          end

          # The model the serving provider ran, which differs from the requested
          # one when a `models` fallback was used.
          #
          # @return [String, nil]
          def served_model
            selected_endpoint&.dig('model')
          end

          # How many providers were tried before one succeeded.
          #
          # Greater than one means the preferred provider failed and routing
          # moved on -- the signal that a preference was not honoured.
          #
          # @return [Integer, nil]
          def routing_attempts
            routing_metadata&.dig('attempt')
          end

          # The raw `openrouter_metadata` block, when present.
          #
          # The shape is explicitly additive, so read it defensively rather than
          # assuming any particular key survives.
          #
          # @return [Hash, nil]
          def routing_metadata
            meta = @raw_response['openrouter_metadata']
            meta.is_a?(Hash) ? meta : nil
          end

          def to_s
            choices.map(&:to_s).join(' ')
          end

          private

          # Prefers the endpoint marked selected, falling back to the last
          # recorded attempt: a partial metadata block can carry one without the
          # other.
          def selected_endpoint
            meta = routing_metadata
            return nil unless meta

            available = meta.dig('endpoints', 'available')
            if available.is_a?(Array)
              selected = available.find { |e| e.is_a?(Hash) && e['selected'] }
              return selected if selected
            end

            attempts = meta['attempts']
            attempts.is_a?(Array) ? attempts.last : nil
          end
        end

        # Choice wrapper for OpenRouter API responses.
        class OpenRouterChoice
          attr_reader :message, :finish_reason

          def initialize(choice)
            @message = OpenRouterMessage.new(choice['message'])
            @finish_reason = choice['finish_reason']
          end

          def to_s
            @message.to_s
          end
        end

        # Message wrapper for OpenRouter API responses.
        class OpenRouterMessage
          attr_reader :role, :content

          def initialize(message)
            @role = message['role']
            @content = message['content']
          end

          def to_s
            @content
          end
        end

        # Stream response wrapper for OpenRouter API streaming responses.
        class OpenRouterStreamResponse
          attr_reader :choices

          def initialize(parsed)
            @choices = OpenRouterStreamChoice.new(parsed['choices'])
          end

          def to_s
            @choices.to_s
          end
        end

        # Embedding response wrapper for OpenRouter API embedding responses.
        class OpenRouterEmbeddingResponse
          attr_reader :embedding

          def initialize(data)
            @embedding = data.dig('data', 0, 'embedding')
          end

          def to_a
            @embedding
          end
        end

        # Stream choice wrapper for OpenRouter API streaming responses.
        class OpenRouterStreamChoice
          attr_reader :delta, :finish_reason

          def initialize(choice)
            @choice = [choice].flatten.first
            @delta = OpenRouterStreamDelta.new(@choice['delta'])
            @finish_reason = @choice['finish_reason']
          end

          def to_s
            @delta.to_s
          end
        end

        # Stream delta wrapper for OpenRouter API streaming responses.
        class OpenRouterStreamDelta
          attr_reader :role, :content

          def initialize(delta)
            @role = delta['role']
            @content = delta['content']
          end

          def to_s
            @content || ''
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
