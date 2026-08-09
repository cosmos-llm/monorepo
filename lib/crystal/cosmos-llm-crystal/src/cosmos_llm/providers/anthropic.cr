require "./base"

module CosmosLLM
  module Providers
    # Provider implementation for the Anthropic Messages API.
    #
    # Supports chat completions (with optional system message), tool calling,
    # and model listing. Reads the API key from `ANTHROPIC_API_KEY` or
    # `CLLM__ANTHROPIC__API_KEY` when none is supplied at construction.
    #
    # ```
    # provider = CosmosLLM::Providers::Anthropic.new("sk-ant-test")
    # req = CosmosLLM::CompletionRequest.new(
    #   "claude-sonnet-4-6",
    #   [CosmosLLM::Message.user("Hello!")]
    # )
    # provider.completion(req)
    # ```
    class Anthropic < Base
      BASE_URL          = "https://api.anthropic.com/v1"
      ANTHROPIC_VERSION = "2023-06-01"

      # Default `max_tokens` when the request does not set one. The Anthropic
      # Messages API requires this field.
      DEFAULT_MAX_TOKENS = 1024

      # Static list of known Claude models, returned by `#models`.
      #
      # Anthropic's models endpoint requires a paid account; this list is used
      # as a fallback so callers can enumerate models without an active
      # subscription.
      KNOWN_MODELS = %w[
        claude-opus-4-8
        claude-sonnet-4-6
        claude-haiku-4-5-20251001
        claude-3-5-sonnet-20241022
        claude-3-5-haiku-20241022
        claude-3-opus-20240229
        claude-3-haiku-20240307
      ]

      # Creates a new Anthropic provider.
      #
      # When *api_key* is `nil`, falls back to the `ANTHROPIC_API_KEY` or
      # `CLLM__ANTHROPIC__API_KEY` environment variables.
      def initialize(api_key : String? = nil)
        @api_key = api_key || ENV["ANTHROPIC_API_KEY"]? || ENV["CLLM__ANTHROPIC__API_KEY"]?
      end

      # :inherit:
      def completion(request : CompletionRequest) : CompletionResponse
        system, chat = self.class.split_system(request.messages)

        body = JSON.build do |json|
          json.object do
            json.field "model", request.model
            json.field "messages", chat
            json.field "max_tokens", request.max_tokens || DEFAULT_MAX_TOKENS
            json.field "system", system if system
            if temperature = request.temperature
              json.field "temperature", temperature
            end
            if top_p = request.top_p
              json.field "top_p", top_p
            end
            if stop = request.stop
              json.field "stop_sequences", stop
            end
            if tools = request.tools
              json.field "tools", tools
            end
            if tool_choice = request.tool_choice
              json.field "tool_choice", tool_choice
            end
          end
        end

        parsed = request_json("POST", "#{BASE_URL}/messages", auth_headers, body)
        self.class.map_response(parsed)
      end

      # :inherit:
      #
      # Returns `KNOWN_MODELS`; see that constant for why the list is static.
      def models : Array(String)
        KNOWN_MODELS.dup
      end

      # Splits messages into an optional system string and the remaining chat
      # messages.
      #
      # Anthropic's API separates the system prompt from the message array.
      # When several system messages are present, the last one wins.
      #
      # ```
      # system, chat = CosmosLLM::Providers::Anthropic.split_system([
      #   CosmosLLM::Message.system("be helpful"),
      #   CosmosLLM::Message.user("hi"),
      # ])
      # system    # => "be helpful"
      # chat.size # => 1
      # ```
      def self.split_system(messages : Array(Message)) : {String?, Array(Message)}
        system = nil.as(String?)
        chat = [] of Message

        messages.each do |message|
          if message.role == "system"
            system = message.content
          else
            chat << message
          end
        end

        {system, chat}
      end

      # Converts an Anthropic Messages body into a `CompletionResponse`.
      def self.map_response(body : JSON::Any) : CompletionResponse
        blocks = body["content"]?.try(&.as_a?) || [] of JSON::Any

        text = blocks
          .select { |block| block["type"]?.try(&.as_s?) == "text" }
          .join { |block| block["text"]?.try(&.as_s?) || "" }

        tool_calls = blocks
          .select { |block| block["type"]?.try(&.as_s?) == "tool_use" }
          .map do |block|
            ToolCall.new(
              id: block["id"]?.try(&.as_s?) || "",
              name: block["name"]?.try(&.as_s?) || "",
              input: block["input"]? || JSON::Any.new(nil),
            )
          end

        choice = Choice.new(
          index: 0,
          message: Message.assistant(text),
          finish_reason: body["stop_reason"]?.try(&.as_s?),
          tool_calls: tool_calls,
        )

        CompletionResponse.new(
          choices: [choice],
          id: body["id"]?.try(&.as_s?),
          model: body["model"]?.try(&.as_s?),
          usage: map_usage(body["usage"]?),
        )
      end

      private def self.map_usage(raw : JSON::Any?) : Usage?
        usage = raw.try(&.as_h?)
        return nil unless usage

        input_tokens = usage["input_tokens"]?.try(&.as_i?) || 0
        output_tokens = usage["output_tokens"]?.try(&.as_i?) || 0

        Usage.new(
          prompt_tokens: input_tokens,
          completion_tokens: output_tokens,
          total_tokens: input_tokens + output_tokens,
        )
      end

      # Returns the resolved API key, raising when none is configured.
      def resolved_key : String
        @api_key || raise AuthenticationError.new(
          "Anthropic API key not set. Export ANTHROPIC_API_KEY or CLLM__ANTHROPIC__API_KEY."
        )
      end

      private def auth_headers : HTTP::Headers
        HTTP::Headers{
          "x-api-key"         => resolved_key,
          "anthropic-version" => ANTHROPIC_VERSION,
        }
      end
    end
  end
end
