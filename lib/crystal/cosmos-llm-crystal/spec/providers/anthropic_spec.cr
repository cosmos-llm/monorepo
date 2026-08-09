require "../spec_helper"

describe CosmosLLM::Providers::Anthropic do
  it "does not support streaming" do
    CosmosLLM::Providers::Anthropic.new("key").supports_streaming?.should be_false
  end

  it "raises an authentication error when no key is configured" do
    with_clean_env do
      expect_raises(CosmosLLM::AuthenticationError, /ANTHROPIC_API_KEY/) do
        CosmosLLM::Providers::Anthropic.new.resolved_key
      end
    end
  end

  it "returns the known model list" do
    models = CosmosLLM::Providers::Anthropic.new("key").models

    models.should contain("claude-sonnet-4-6")
    models.should contain("claude-opus-4-8")
  end

  it "separates the system message from the chat messages" do
    system, chat = CosmosLLM::Providers::Anthropic.split_system([
      CosmosLLM::Message.system("be helpful"),
      CosmosLLM::Message.user("hi"),
      CosmosLLM::Message.assistant("hello"),
    ])

    system.should eq("be helpful")
    chat.size.should eq(2)
    chat.map(&.role).should eq(["user", "assistant"])
  end

  it "returns a nil system message when there is none" do
    system, chat = CosmosLLM::Providers::Anthropic.split_system([CosmosLLM::Message.user("hi")])

    system.should be_nil
    chat.size.should eq(1)
  end

  it "maps a messages body" do
    body = JSON.parse(<<-JSON)
      {
        "id": "msg_1",
        "model": "claude-sonnet-4-6",
        "content": [{ "type": "text", "text": "Hi there!" }],
        "stop_reason": "end_turn",
        "usage": { "input_tokens": 8, "output_tokens": 4 }
      }
      JSON

    resp = CosmosLLM::Providers::Anthropic.map_response(body)

    resp.id.should eq("msg_1")
    resp.content.should eq("Hi there!")
    resp.tool_use?.should be_false
    resp.choices.first.finish_reason.should eq("end_turn")
    resp.usage.not_nil!.total_tokens.should eq(12)
  end

  it "concatenates multiple text blocks" do
    body = JSON.parse(<<-JSON)
      {
        "content": [
          { "type": "text", "text": "Hello, " },
          { "type": "text", "text": "world!" }
        ]
      }
      JSON

    CosmosLLM::Providers::Anthropic.map_response(body).text.should eq("Hello, world!")
  end

  it "parses tool_use blocks alongside text" do
    body = JSON.parse(<<-JSON)
      {
        "id": "msg_2",
        "model": "claude-sonnet-4-6",
        "content": [
          { "type": "text", "text": "Let me check that." },
          {
            "type": "tool_use",
            "id": "toolu_1",
            "name": "get_weather",
            "input": { "city": "Boston" }
          }
        ],
        "stop_reason": "tool_use",
        "usage": { "input_tokens": 10, "output_tokens": 6 }
      }
      JSON

    resp = CosmosLLM::Providers::Anthropic.map_response(body)

    resp.tool_use?.should be_true
    resp.text.should eq("Let me check that.")

    call = resp.tool_calls.first
    call.id.should eq("toolu_1")
    call.name.should eq("get_weather")
    call.input.should eq(JSON.parse(%({"city":"Boston"})))
  end

  it "returns an empty text choice for an empty content array" do
    resp = CosmosLLM::Providers::Anthropic.map_response(JSON.parse(%({"content":[]})))

    resp.choices.size.should eq(1)
    resp.text.should eq("")
    resp.usage.should be_nil
  end
end
