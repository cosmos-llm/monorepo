require "./base"

module CosmosLLM
  module Providers
    # Provider implementation for the OpenAI API.
    #
    # Supports chat completions, tool calling, and model listing. Reads the
    # API key from `OPENAI_API_KEY` or `CLLM__OPENAI__API_KEY` when none is
    # supplied at construction.
    #
    # ```
    # provider = CosmosLLM::Providers::OpenAI.new("sk-test")
    # req = CosmosLLM::CompletionRequest.new("gpt-4o", [CosmosLLM::Message.user("Hello")])
    # provider.completion(req)
    # ```
    class OpenAI < Base
      BASE_URL = "https://api.openai.com/v1"

      # Creates a new OpenAI provider.
      #
      # When *api_key* is `nil`, falls back to the `OPENAI_API_KEY` or
      # `CLLM__OPENAI__API_KEY` environment variables.
      def initialize(api_key : String? = nil)
        @api_key = api_key || ENV["OPENAI_API_KEY"]? || ENV["CLLM__OPENAI__API_KEY"]?
      end

      # :inherit:
      def completion(request : CompletionRequest) : CompletionResponse
        body = JSON.build do |json|
          json.object do
            json.field "model", request.model
            json.field "messages", request.messages
            if temperature = request.temperature
              json.field "temperature", temperature
            end
            if max_tokens = request.max_tokens
              json.field "max_tokens", max_tokens
            end
            if top_p = request.top_p
              json.field "top_p", top_p
            end
            if stop = request.stop
              json.field "stop", stop
            end
            if tools = request.tools
              json.field "tools", tools
            end
            if tool_choice = request.tool_choice
              json.field "tool_choice", tool_choice
            end
          end
        end

        parsed = request_json("POST", "#{BASE_URL}/chat/completions", auth_headers, body)
        self.class.map_response(parsed)
      end

      # :inherit:
      def models : Array(String)
        parsed = request_json("GET", "#{BASE_URL}/models", auth_headers)

        data = parsed["data"]?.try(&.as_a?) || [] of JSON::Any
        data.compact_map { |model| model["id"]?.try(&.as_s?) }
      end

      # :inherit:
      def supports_streaming? : Bool
        true
      end

      # Converts an OpenAI chat completion body into a `CompletionResponse`.
      def self.map_response(body : JSON::Any) : CompletionResponse
        choices_json = body["choices"]?.try(&.as_a?)
        raise InvalidResponseError.new("missing 'choices' field") unless choices_json

        choices = choices_json.map_with_index do |choice, position|
          message = choice["message"]? || JSON::Any.new(nil)

          Choice.new(
            index: choice["index"]?.try(&.as_i?) || position,
            message: Message.new(
              message["role"]?.try(&.as_s?) || "assistant",
              message["content"]?.try(&.as_s?) || "",
            ),
            finish_reason: choice["finish_reason"]?.try(&.as_s?),
            tool_calls: map_tool_calls(message["tool_calls"]?),
          )
        end

        CompletionResponse.new(
          choices: choices,
          id: body["id"]?.try(&.as_s?),
          model: body["model"]?.try(&.as_s?),
          usage: map_usage(body["usage"]?),
        )
      end

      # Normalizes OpenAI `tool_calls` entries, parsing the JSON-encoded
      # `arguments` string into a `JSON::Any` object.
      private def self.map_tool_calls(raw : JSON::Any?) : Array(ToolCall)
        calls = raw.try(&.as_a?)
        return [] of ToolCall unless calls

        calls.map do |call|
          function = call["function"]? || JSON::Any.new(nil)
          arguments = function["arguments"]?.try(&.as_s?) || "{}"

          input = begin
            JSON.parse(arguments)
          rescue JSON::ParseException
            JSON::Any.new(nil)
          end

          ToolCall.new(
            id: call["id"]?.try(&.as_s?) || "",
            name: function["name"]?.try(&.as_s?) || "",
            input: input,
          )
        end
      end

      private def self.map_usage(raw : JSON::Any?) : Usage?
        usage = raw.try(&.as_h?)
        return nil unless usage

        Usage.new(
          prompt_tokens: usage["prompt_tokens"]?.try(&.as_i?) || 0,
          completion_tokens: usage["completion_tokens"]?.try(&.as_i?) || 0,
          total_tokens: usage["total_tokens"]?.try(&.as_i?) || 0,
        )
      end

      # Returns the resolved API key, raising when none is configured.
      def resolved_key : String
        @api_key || raise AuthenticationError.new(
          "OpenAI API key not set. Export OPENAI_API_KEY or CLLM__OPENAI__API_KEY."
        )
      end

      private def auth_headers : HTTP::Headers
        HTTP::Headers{"Authorization" => "Bearer #{resolved_key}"}
      end
    end
  end
end
