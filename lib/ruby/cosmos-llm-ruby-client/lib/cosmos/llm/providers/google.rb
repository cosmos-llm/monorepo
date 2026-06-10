# frozen_string_literal: true

# This file implements the Google provider for accessing Google's Gemini language models through their API, providing completion capabilities with authentication handling, error management, and response normalization. It establishes HTTP connections to Google's Generative Language API endpoint, processes generateContent requests with text content, handles various API error responses, and includes comprehensive response classes to format Google's API responses into a consistent interface.

require 'cosmos/llm/http_client'
require 'json'
require 'cosmos/llm/errors'
require 'cosmos/llm/providers/base'

module Cosmos
  module Llm
    module Providers
      # Google Generative AI provider for accessing Gemini language models.
      #
      # Provides completion, embedding, and streaming capabilities with proper
      # error handling and response normalization for Google's Generative Language API.
      class Google < Cosmos::Llm::Providers::Base
        BASE_URL = 'https://generativelanguage.googleapis.com'

        def default_api_key
          begin
            Cosmos::Llm.configuration.google&.api_key
          rescue NoMethodError
            nil
          end || ENV['GOOGLE_API_KEY']
        end

        attr_accessor :api_key

        def initialize(api_key: nil)
          @api_key = api_key || default_api_key
          @conn = Cosmos::Llm::HttpClient.new(url: BASE_URL)
        end

        def completion(options)
          model = options[:model]
          url = "/v1beta/models/#{model}:generateContent?key=#{@api_key}"

          # Transform options to Google's format
          request_body = transform_options(options)

          response = @conn.post(url) do |req|
            req.body = request_body
          end

          handle_response(response)
        end

        def embedding(model:, input:, **_options)
          url = "/v1beta/models/#{model}:embedContent?key=#{@api_key}"

          request_body = {
            content: {
              parts: [{ text: input }]
            }
          }

          response = @conn.post(url) do |req|
            req.body = request_body
          end

          handle_response(response, GoogleEmbeddingResponse)
        end

        def models
          # Google doesn't provide a public models API, so return hardcoded list
          [
            'gemini-1.5-flash',
            'gemini-1.5-flash-001',
            'gemini-1.5-flash-002',
            'gemini-1.5-flash-8b',
            'gemini-1.5-flash-8b-001',
            'gemini-1.5-flash-8b-latest',
            'gemini-1.5-flash-latest',
            'gemini-1.5-pro',
            'gemini-1.5-pro-001',
            'gemini-1.5-pro-002',
            'gemini-1.5-pro-latest',
            'gemini-2.0-flash',
            'gemini-2.0-flash-001',
            'gemini-2.0-flash-exp',
            'gemini-2.0-flash-lite',
            'gemini-2.0-flash-lite-001',
            'gemini-2.0-flash-live-001',
            'gemini-2.0-flash-preview-image-generation',
            'gemini-2.5-flash',
            'gemini-2.5-flash-exp-native-audio-thinking-dialog',
            'gemini-2.5-flash-lite',
            'gemini-2.5-flash-lite-06-17',
            'gemini-2.5-flash-preview-05-20',
            'gemini-2.5-flash-preview-native-audio-dialog',
            'gemini-2.5-flash-preview-tts',
            'gemini-2.5-pro',
            'gemini-2.5-pro-preview-tts',
            'gemini-live-2.5-flash-preview',
            'text-embedding-004',
            'text-multilingual-embedding-002'
          ]
        end

        def self.stream?
          true
        end
        def stream(options, &block)
          model = options[:model]
          url = "/v1beta/models/#{model}:streamGenerateContent?key=#{@api_key}&alt=sse"

          request_body = transform_options(options)

          response = @conn.post_stream(url) do |stream|
            stream.on_chunk { |chunk| block.call(GoogleStreamResponse.new(chunk)) }
            stream.headers['Accept'] = 'text/event-stream'
            stream.body = request_body
          end

          handle_response(response)
        end

        private

        def transform_options(options)
          messages = options[:messages] || []
          system_messages = messages.select { |m| symbolic(m, :role) == 'system' }
          conversation_messages = messages.reject { |m| symbolic(m, :role) == 'system' }

          body = {
            contents: conversation_messages.map do |msg|
              {
                role: symbolic(msg, :role) == 'assistant' ? 'model' : 'user',
                parts: content_to_parts(symbolic(msg, :content))
              }
            end
          }

          if system_messages.any?
            body[:systemInstruction] = {
              parts: [{ text: system_messages.map { |m| text_of(symbolic(m, :content)) }.join("\n") }]
            }
          end

          tools = transform_tools(options[:tools])
          body[:tools] = tools if tools

          generation_config = {}
          generation_config[:temperature] = options[:temperature] if options[:temperature]
          generation_config[:maxOutputTokens] = options[:max_tokens] if options[:max_tokens]
          generation_config[:topP] = options[:top_p] if options[:top_p]
          generation_config[:topK] = options[:top_k] if options[:top_k]

          body[:generationConfig] = generation_config unless generation_config.empty?

          body
        end

        # Fetch a key from a message hash regardless of symbol/string keys.
        def symbolic(hash, key)
          return nil unless hash.is_a?(Hash)

          hash[key] || hash[key.to_s]
        end

        # Convert message content into Gemini `parts`. Content may be a plain
        # string, or an array of normalized blocks (text / tool_use / tool_result)
        # in the shape an agent loop accumulates across turns.
        def content_to_parts(content)
          return [{ text: content.to_s }] unless content.is_a?(Array)

          parts = content.map { |block| block_to_part(block) }.compact
          parts.empty? ? [{ text: '' }] : parts
        end

        def block_to_part(block)
          return { text: block.to_s } unless block.is_a?(Hash)

          type = block['type'] || block[:type]
          case type
          when 'tool_use'
            {
              functionCall: {
                name: block['name'] || block[:name],
                args: block['input'] || block[:input] || {}
              }
            }
          when 'tool_result'
            {
              functionResponse: {
                name: block['name'] || block[:name] || block['tool_use_id'] || block[:tool_use_id],
                response: { result: stringify(block['content'] || block[:content]) }
              }
            }
          else
            { text: block['text'] || block[:text] || '' }
          end
        end

        def text_of(content)
          return content.to_s unless content.is_a?(Array)

          content.map { |b| b.is_a?(Hash) ? (b['text'] || b[:text]) : b }.compact.join("\n")
        end

        def stringify(value)
          value.is_a?(String) ? value : JSON.generate(value)
        end

        # Translate provider-neutral tool schemas (Anthropic-style: name,
        # description, input_schema) into Gemini functionDeclarations.
        def transform_tools(tools)
          return nil if tools.nil? || tools.empty?

          declarations = tools.map do |tool|
            name        = tool[:name] || tool['name']
            description = tool[:description] || tool['description']
            schema      = tool[:input_schema] || tool['input_schema'] ||
                          tool[:parameters] || tool['parameters']
            decl = { name: name }
            decl[:description] = description if description
            decl[:parameters] = sanitize_schema(schema) if schema && !schema_empty?(schema)
            decl
          end

          [{ functionDeclarations: declarations }]
        end

        # Gemini rejects empty object schemas; drop parameters with no properties.
        def schema_empty?(schema)
          props = schema[:properties] || schema['properties']
          props.nil? || props.empty?
        end

        # Gemini's schema dialect is a strict subset of JSON Schema; strip keys it
        # does not accept so a tool definition written for Anthropic still loads.
        def sanitize_schema(schema)
          return schema unless schema.is_a?(Hash)

          allowed = %w[type description properties required items enum format nullable]
          schema.each_with_object({}) do |(k, v), out|
            key = k.to_s
            next unless allowed.include?(key)

            out[key] =
              case key
              when 'properties'
                (v || {}).transform_values { |sub| sanitize_schema(sub) }
              when 'items'
                sanitize_schema(v)
              else
                v
              end
          end
        end

        def handle_response(response, response_class = GoogleResponse)
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

        # Response object for Google Generative AI API responses.
        #
        # Wraps the raw response and provides a consistent interface for accessing
        # candidate content and metadata.
        class GoogleResponse
          attr_reader :raw_response

          def initialize(response)
            @raw_response = response
          end

          def choices
            [GoogleChoice.new(@raw_response['candidates']&.first)]
          end

          def parts
            @raw_response.dig('candidates', 0, 'content', 'parts') || []
          end

          def tool_use?
            parts.any? { |p| p.is_a?(Hash) && p['functionCall'] }
          end

          # Provider-neutral assistant text: concatenation of all text parts.
          def text
            parts.filter_map { |p| p['text'] if p.is_a?(Hash) }.join
          end

          # Provider-neutral tool calls: array of { 'id', 'name', 'input' } where
          # input is a PARSED hash. Gemini gives no call id, so synthesize a stable
          # one from position + name so tool_result correlation still works.
          def tool_calls
            parts.each_with_index.filter_map do |p, i|
              next unless p.is_a?(Hash) && (fc = p['functionCall'])

              name = fc['name']
              {
                'id'    => "call_#{i}_#{name}",
                'name'  => name,
                'input' => fc['args'] || {}
              }
            end
          end

          def to_s
            choices.map(&:to_s).join(' ')
          end
        end

        # Represents a single candidate choice in a Google response.
        #
        # Contains the message content from the candidate.
        class GoogleChoice
          attr_reader :message

          def initialize(candidate)
            @message = GoogleMessage.new(candidate&.dig('content', 'parts') || [])
          end

          def to_s
            @message.to_s
          end
        end

        # Represents a message in a Google conversation.
        #
        # Messages contain text content concatenated from all text parts.
        class GoogleMessage
          attr_reader :content

          def initialize(parts)
            parts = [parts] unless parts.is_a?(Array)
            @content = parts.filter_map { |p| p.is_a?(Hash) ? p['text'] : nil }.join
          end

          def to_s
            @content
          end
        end

        # Response object for streaming Google Generative AI chunks.
        #
        # Wraps individual chunks from the streaming response.
        class GoogleStreamResponse
          attr_reader :choices

          def initialize(parsed)
            @choices = [GoogleStreamChoice.new(parsed)]
          end

          def to_s
            @choices.map(&:to_s).join
          end
        end

        # Represents a single choice in a streaming Google response chunk.
        #
        # Contains the delta (incremental content) for the choice.
        class GoogleStreamChoice
          attr_reader :delta

          def initialize(parsed)
            @delta = GoogleStreamDelta.new(parsed.dig('candidates', 0, 'content', 'parts', 0))
          end

          def to_s
            @delta.to_s
          end
        end

        # Represents the incremental content delta in a streaming response.
        #
        # Contains the text content of the delta.
        class GoogleStreamDelta
          attr_reader :content

          def initialize(part)
            @content = part&.dig('text') || ''
          end

          def to_s
            @content
          end
        end

        # Response object for Google embedding API responses.
        #
        # Wraps embedding data and provides array access to the vector representation.
        class GoogleEmbeddingResponse
          attr_reader :embedding

          def initialize(data)
            @embedding = data.dig('embedding', 'values')
          end

          def to_a
            @embedding
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
