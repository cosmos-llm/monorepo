require "http/client"
require "json"
require "uri"

require "../error"
require "../types"

module CosmosLLM
  module Providers
    # The common interface every provider must implement.
    #
    # Providers are typically constructed with their API key and then called
    # through a `CosmosLLM::Client`. Subclass `Base` to add support for a new
    # LLM backend.
    abstract class Base
      # Sends a completion request and returns the full response.
      #
      # Raises a `CosmosLLM::Error` subclass on authentication failure,
      # network error, rate limiting, or an invalid response.
      abstract def completion(request : CompletionRequest) : CompletionResponse

      # Returns the list of model identifiers available from this provider.
      abstract def models : Array(String)

      # Returns `true` if this provider supports streaming completions.
      def supports_streaming? : Bool
        false
      end

      # Issues a JSON request and returns the parsed body.
      #
      # Non-2xx responses are converted to the appropriate `CosmosLLM::Error`
      # subclass by `#handle_error`.
      protected def request_json(
        method : String,
        url : String,
        headers : HTTP::Headers,
        body : String? = nil,
      ) : JSON::Any
        headers["Content-Type"] = "application/json" if body

        response = HTTP::Client.exec(method, URI.parse(url), headers: headers, body: body)

        parsed = begin
          JSON.parse(response.body)
        rescue ex : JSON::ParseException
          raise InvalidResponseError.new(
            "could not parse provider response (status #{response.status_code}): #{ex.message}"
          )
        end

        return parsed if (200..299).includes?(response.status_code)

        raise handle_error(response.status_code, parsed)
      end

      # Maps an HTTP status code and error body to a `CosmosLLM::Error`.
      protected def handle_error(status : Int32, body : JSON::Any) : Error
        message = body.dig?("error", "message").try(&.as_s?) || "unknown error"

        case status
        when 401      then AuthenticationError.new(message)
        when 429      then RateLimitError.new(message)
        when 400, 404 then InvalidRequestError.new(message)
        when .>=(500) then ServerError.new(message)
        else               InvalidResponseError.new("unexpected status #{status}: #{message}")
        end
      end
    end
  end
end
