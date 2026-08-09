require "../spec_helper"

describe CosmosLLM::Providers::OpenAI do
  it "supports streaming" do
    CosmosLLM::Providers::OpenAI.new("key").supports_streaming?.should be_true
  end

  it "raises an authentication error when no key is configured" do
    with_clean_env do
      expect_raises(CosmosLLM::AuthenticationError, /OPENAI_API_KEY/) do
        CosmosLLM::Providers::OpenAI.new.resolved_key
      end
    end
  end

  it "reads the key from the environment" do
    with_clean_env do
      ENV["OPENAI_API_KEY"] = "sk-from-env"

      CosmosLLM::Providers::OpenAI.new.resolved_key.should eq("sk-from-env")
    end
  end

  it "prefers CLLM__OPENAI__API_KEY when OPENAI_API_KEY is absent" do
    with_clean_env do
      ENV["CLLM__OPENAI__API_KEY"] = "sk-namespaced"

      CosmosLLM::Providers::OpenAI.new.resolved_key.should eq("sk-namespaced")
    end
  end

  it "maps a chat completion body" do
    body = JSON.parse(<<-JSON)
      {
        "id": "chatcmpl-1",
        "model": "gpt-4o",
        "choices": [{
          "index": 0,
          "message": { "role": "assistant", "content": "Hello!" },
          "finish_reason": "stop"
        }],
        "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 }
      }
      JSON

    resp = CosmosLLM::Providers::OpenAI.map_response(body)

    resp.id.should eq("chatcmpl-1")
    resp.model.should eq("gpt-4o")
    resp.content.should eq("Hello!")
    resp.tool_use?.should be_false
    resp.choices.first.finish_reason.should eq("stop")
    resp.usage.not_nil!.total_tokens.should eq(15)
  end

  it "parses tool calls, decoding the JSON arguments string" do
    body = JSON.parse(<<-JSON)
      {
        "id": "chatcmpl-2",
        "model": "gpt-4o",
        "choices": [{
          "index": 0,
          "message": {
            "role": "assistant",
            "content": null,
            "tool_calls": [{
              "id": "call_1",
              "type": "function",
              "function": {
                "name": "get_weather",
                "arguments": "{\\"city\\":\\"Boston\\"}"
              }
            }]
          },
          "finish_reason": "tool_calls"
        }],
        "usage": { "prompt_tokens": 12, "completion_tokens": 8, "total_tokens": 20 }
      }
      JSON

    resp = CosmosLLM::Providers::OpenAI.map_response(body)

    resp.tool_use?.should be_true
    resp.text.should eq("")

    call = resp.tool_calls.first
    call.id.should eq("call_1")
    call.name.should eq("get_weather")
    call.input.should eq(JSON.parse(%({"city":"Boston"})))
  end

  it "tolerates malformed tool call arguments" do
    body = JSON.parse(<<-JSON)
      {
        "choices": [{
          "index": 0,
          "message": {
            "role": "assistant",
            "tool_calls": [{
              "id": "call_1",
              "function": { "name": "broken", "arguments": "not json" }
            }]
          }
        }]
      }
      JSON

    call = CosmosLLM::Providers::OpenAI.map_response(body).tool_calls.first

    call.name.should eq("broken")
    call.input.raw.should be_nil
  end

  it "omits usage when the body has none" do
    body = JSON.parse(%({"choices":[{"index":0,"message":{"role":"assistant","content":"hi"}}]}))

    CosmosLLM::Providers::OpenAI.map_response(body).usage.should be_nil
  end

  it "raises when choices are missing" do
    expect_raises(CosmosLLM::InvalidResponseError, /choices/) do
      CosmosLLM::Providers::OpenAI.map_response(JSON.parse(%({"id":"x"})))
    end
  end
end
