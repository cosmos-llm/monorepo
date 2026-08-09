require "json"

module CosmosLLM
  # A single message in a conversation.
  #
  # ```
  # CosmosLLM::Message.user("What is 2 + 2?")
  # ```
  struct Message
    include JSON::Serializable

    # Role of the message author: `"system"`, `"user"`, or `"assistant"`.
    getter role : String

    # Text content of the message.
    getter content : String

    # Creates a new message.
    #
    # * *role* — one of `"system"`, `"user"`, or `"assistant"`.
    # * *content* — message text.
    #
    # ```
    # msg = CosmosLLM::Message.new("user", "Hello!")
    # msg.role # => "user"
    # ```
    def initialize(@role : String, @content : String)
    end

    # Creates a `system` role message.
    #
    # ```
    # CosmosLLM::Message.system("You are a helpful assistant.").role # => "system"
    # ```
    def self.system(content : String) : self
      new("system", content)
    end

    # Creates a `user` role message.
    #
    # ```
    # CosmosLLM::Message.user("What is 2+2?").role # => "user"
    # ```
    def self.user(content : String) : self
      new("user", content)
    end

    # Creates an `assistant` role message.
    #
    # ```
    # CosmosLLM::Message.assistant("4").role # => "assistant"
    # ```
    def self.assistant(content : String) : self
      new("assistant", content)
    end
  end

  # Parameters for a completion or chat request.
  #
  # Setters are chainable, mirroring the Rust crate's builder API:
  #
  # ```
  # req = CosmosLLM::CompletionRequest.new("gpt-4o", [CosmosLLM::Message.user("Hi")])
  #   .with_temperature(0.7)
  #   .with_max_tokens(256)
  # ```
  class CompletionRequest
    # Model identifier, e.g. `"gpt-4o"` or `"claude-3-5-sonnet-20241022"`.
    property model : String

    # Conversation history to send to the provider.
    property messages : Array(Message)

    # Sampling temperature in `[0.0, 2.0]`. Higher values increase randomness.
    property temperature : Float64?

    # Maximum number of tokens to generate.
    property max_tokens : Int32?

    # Nucleus sampling cutoff in `[0.0, 1.0]`.
    property top_p : Float64?

    # Sequences at which the model will stop generating further tokens.
    property stop : Array(String)?

    # Tool schemas the model may call, in provider-specific format.
    property tools : Array(JSON::Any)?

    # Provider-specific tool choice directive (e.g. `"auto"`, `"none"`, or a
    # forced-tool object).
    property tool_choice : JSON::Any?

    # Creates a new `CompletionRequest`.
    #
    # * *model* — model identifier.
    # * *messages* — conversation history.
    #
    # ```
    # req = CosmosLLM::CompletionRequest.new("gpt-4o", [CosmosLLM::Message.user("Hello")])
    # req.model # => "gpt-4o"
    # ```
    def initialize(
      @model : String,
      @messages : Array(Message),
      @temperature : Float64? = nil,
      @max_tokens : Int32? = nil,
      @top_p : Float64? = nil,
      @stop : Array(String)? = nil,
      @tools : Array(JSON::Any)? = nil,
      @tool_choice : JSON::Any? = nil,
    )
    end

    # Sets the sampling temperature and returns `self`.
    #
    # ```
    # req.with_temperature(0.5).temperature # => 0.5
    # ```
    def with_temperature(value : Float64) : self
      @temperature = value
      self
    end

    # Sets the maximum number of tokens to generate and returns `self`.
    #
    # ```
    # req.with_max_tokens(100).max_tokens # => 100
    # ```
    def with_max_tokens(value : Int32) : self
      @max_tokens = value
      self
    end

    # Sets the top-p nucleus sampling value and returns `self`.
    #
    # ```
    # req.with_top_p(0.9).top_p # => 0.9
    # ```
    def with_top_p(value : Float64) : self
      @top_p = value
      self
    end

    # Adds stop sequences and returns `self`.
    #
    # ```
    # req.with_stop(["END"]).stop # => ["END"]
    # ```
    def with_stop(value : Array(String)) : self
      @stop = value
      self
    end

    # Sets the tool schemas the model may call and returns `self`.
    #
    # Each entry is a provider-specific schema. Accepts raw `Hash` values for
    # convenience; they are converted to `JSON::Any`.
    #
    # ```
    # req.with_tools([{"name" => "echo"}])
    # ```
    def with_tools(value : Array(JSON::Any)) : self
      @tools = value
      self
    end

    # :ditto:
    def with_tools(value : Array) : self
      @tools = value.map { |t| JSON.parse(t.to_json) }
      self
    end

    # Sets the provider-specific tool choice directive and returns `self`.
    #
    # ```
    # req.with_tool_choice("auto")
    # ```
    def with_tool_choice(value : JSON::Any) : self
      @tool_choice = value
      self
    end

    # :ditto:
    def with_tool_choice(value) : self
      @tool_choice = JSON.parse(value.to_json)
      self
    end
  end

  # Token usage reported by the provider.
  struct Usage
    include JSON::Serializable

    # Tokens consumed by the prompt.
    getter prompt_tokens : Int32

    # Tokens generated in the completion.
    getter completion_tokens : Int32

    # Total tokens (prompt + completion).
    getter total_tokens : Int32

    def initialize(@prompt_tokens : Int32, @completion_tokens : Int32, @total_tokens : Int32)
    end
  end

  # A single tool call requested by the model, normalized across providers.
  #
  # `input` is always a parsed JSON value regardless of whether the source
  # provider encoded arguments as a JSON string (OpenAI) or a native object
  # (Anthropic).
  struct ToolCall
    include JSON::Serializable

    # Provider-assigned identifier for this call, used to correlate a tool
    # result back to the request.
    getter id : String

    # Name of the tool being called.
    getter name : String

    # Parsed arguments to pass to the tool.
    getter input : JSON::Any

    def initialize(@id : String, @name : String, @input : JSON::Any)
    end
  end

  # A single choice returned by the provider.
  struct Choice
    include JSON::Serializable

    # Zero-based index of this choice.
    getter index : Int32

    # The generated message.
    getter message : Message

    # Reason the generation stopped (e.g. `"stop"`, `"length"`).
    getter finish_reason : String?

    # Tool calls requested by the model, if any.
    @[JSON::Field(emit_null: false)]
    getter tool_calls : Array(ToolCall) = [] of ToolCall

    def initialize(
      @index : Int32,
      @message : Message,
      @finish_reason : String? = nil,
      @tool_calls : Array(ToolCall) = [] of ToolCall,
    )
    end
  end

  # The response from a completion request.
  struct CompletionResponse
    include JSON::Serializable

    # Provider-assigned response identifier.
    getter id : String?

    # Model that produced the response.
    getter model : String?

    # One or more completion choices.
    getter choices : Array(Choice)

    # Token usage statistics, if provided.
    getter usage : Usage?

    def initialize(
      @choices : Array(Choice),
      @id : String? = nil,
      @model : String? = nil,
      @usage : Usage? = nil,
    )
    end

    # Returns the text content of the first choice, or `nil` when there are no
    # choices.
    #
    # ```
    # resp.content # => "Hello!"
    # ```
    def content : String?
      @choices.first?.try(&.message.content)
    end

    # Returns the assistant text of the first choice, or an empty string if
    # there are no choices.
    #
    # Unlike `#content`, this never returns `nil` — it mirrors the Ruby
    # client's provider-neutral `text` accessor, which callers can use
    # unconditionally in a tool-calling loop.
    #
    # ```
    # CosmosLLM::CompletionResponse.new([] of CosmosLLM::Choice).text # => ""
    # ```
    def text : String
      content || ""
    end

    # Returns `true` if the first choice requested any tool calls.
    def tool_use? : Bool
      !tool_calls.empty?
    end

    # Returns the tool calls requested by the first choice, or an empty array
    # if there are no choices or no tool calls.
    def tool_calls : Array(ToolCall)
      @choices.first?.try(&.tool_calls) || [] of ToolCall
    end
  end

  # A delta chunk delivered during a streaming response.
  struct StreamChunk
    # Incremental text fragment for this chunk.
    getter delta : String

    # Set when the stream is complete.
    getter finish_reason : String?

    def initialize(@delta : String, @finish_reason : String? = nil)
    end
  end
end
