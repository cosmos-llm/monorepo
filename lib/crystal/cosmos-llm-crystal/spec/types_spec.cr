require "./spec_helper"

describe CosmosLLM::Message do
  it "builds messages for each role" do
    CosmosLLM::Message.system("sys").role.should eq("system")
    CosmosLLM::Message.user("usr").role.should eq("user")
    CosmosLLM::Message.assistant("asst").role.should eq("assistant")
  end

  it "serializes to the provider wire format" do
    json = CosmosLLM::Message.user("hi").to_json
    JSON.parse(json).should eq(JSON.parse(%({"role":"user","content":"hi"})))
  end
end

describe CosmosLLM::CompletionRequest do
  it "chains builder methods" do
    req = CosmosLLM::CompletionRequest.new("m", [CosmosLLM::Message.user("hi")])
      .with_temperature(0.5)
      .with_max_tokens(100)
      .with_top_p(0.9)
      .with_stop(["END"])
      .with_tools([{"name" => "echo"}])
      .with_tool_choice("auto")

    req.temperature.should eq(0.5)
    req.max_tokens.should eq(100)
    req.top_p.should eq(0.9)
    req.stop.should eq(["END"])
    req.tools.not_nil!.size.should eq(1)
    req.tool_choice.should eq(JSON::Any.new("auto"))
  end

  it "leaves optional parameters nil by default" do
    req = CosmosLLM::CompletionRequest.new("m", [] of CosmosLLM::Message)

    req.temperature.should be_nil
    req.max_tokens.should be_nil
    req.top_p.should be_nil
    req.stop.should be_nil
    req.tools.should be_nil
    req.tool_choice.should be_nil
  end
end

describe CosmosLLM::CompletionResponse do
  it "reads content from the first choice" do
    resp = CosmosLLM::CompletionResponse.new([
      CosmosLLM::Choice.new(0, CosmosLLM::Message.assistant("hello")),
    ])

    resp.content.should eq("hello")
    resp.text.should eq("hello")
    resp.tool_use?.should be_false
    resp.tool_calls.should be_empty
  end

  it "returns nil content and empty text with no choices" do
    resp = CosmosLLM::CompletionResponse.new([] of CosmosLLM::Choice)

    resp.content.should be_nil
    resp.text.should eq("")
    resp.tool_use?.should be_false
    resp.tool_calls.should be_empty
  end

  it "exposes tool calls from the first choice" do
    call = CosmosLLM::ToolCall.new("1", "echo", JSON.parse(%({"msg":"hi"})))
    resp = CosmosLLM::CompletionResponse.new([
      CosmosLLM::Choice.new(0, CosmosLLM::Message.assistant(""), "tool_calls", [call]),
    ])

    resp.tool_use?.should be_true
    resp.tool_calls.size.should eq(1)
    resp.tool_calls.first.name.should eq("echo")
  end
end
