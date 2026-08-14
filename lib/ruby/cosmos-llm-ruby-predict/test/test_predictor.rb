# frozen_string_literal: true

require_relative 'test_helper'

class TestPredictor < Minitest::Test
  Predict = Cosmos::Llm::Predict::Predict
  Example = Cosmos::Llm::Predict::Example

  def chat_reply(answer: 'four', confidence: '0.9')
    "[[ ## answer ## ]]\n#{answer}\n\n[[ ## confidence ## ]]\n#{confidence}\n\n[[ ## done ## ]]\n"
  end

  def test_returns_a_typed_prediction
    client = FakeClient.new(chat_reply)
    predictor = Predict.new('question -> answer, confidence: number', client: client)

    prediction = predictor.call(question: 'What is 2+2?')

    assert_equal 'four', prediction.answer
    assert_in_delta 0.9, prediction.confidence
    assert_equal predictor.signature, prediction.signature
  end

  def test_prediction_carries_completion_and_usage
    client = FakeClient.new(chat_reply)
    predictor = Predict.new('question -> answer, confidence: number', client: client)

    prediction = predictor.call(question: 'What is 2+2?')

    assert_includes prediction.completion, '[[ ## answer ## ]]'
    assert_equal 10, prediction.usage['input_tokens']
    refute prediction.cached?
  end

  def test_sends_system_and_user_messages
    client = FakeClient.new(chat_reply)
    Predict.new('question -> answer, confidence: number', client: client).call(question: 'Why?')

    messages = client.requests.first[:messages]

    assert_equal %w[system user], messages.map { |m| m[:role] }
  end

  def test_raises_without_a_client
    predictor = Predict.new('question -> answer')

    error = assert_raises(Cosmos::Llm::Predict::ConfigurationError) { predictor.call(question: 'Why?') }
    assert_match(/No LLM client configured/, error.message)
  end

  def test_uses_the_configured_client
    client = FakeClient.new("[[ ## answer ## ]]\nyes\n")
    Cosmos::Llm::Predict.configure { |settings| settings.client = client }

    prediction = Predict.new('question -> answer').call(question: 'Why?')

    assert_equal 'yes', prediction.answer
  end

  def test_validates_inputs_against_the_signature
    predictor = Predict.new('question -> answer', client: FakeClient.new('x'))

    assert_raises(Cosmos::Llm::Signature::MissingFieldError) { predictor.call(wrong: 'Why?') }
  end

  def test_retries_once_after_a_parse_failure
    client = FakeClient.new('malformed output', "[[ ## answer ## ]]\nrecovered\n")
    predictor = Predict.new('question -> answer', client: client)

    prediction = predictor.call(question: 'Why?')

    assert_equal 'recovered', prediction.answer
    assert_equal 2, client.call_count
  end

  def test_retry_includes_the_failed_output_as_context
    client = FakeClient.new('malformed output', "[[ ## answer ## ]]\nrecovered\n")
    Predict.new('question -> answer', client: client).call(question: 'Why?')

    retry_messages = client.requests.last[:messages]

    assert_equal 'malformed output', retry_messages[-2][:content]
    assert_includes retry_messages[-1][:content], 'could not be parsed'
  end

  def test_raises_after_exhausting_retries
    client = FakeClient.new('malformed', 'still malformed')
    predictor = Predict.new('question -> answer', client: client)

    assert_raises(Cosmos::Llm::Predict::ParseError) { predictor.call(question: 'Why?') }
    assert_equal 2, client.call_count
  end

  def test_retry_count_is_configurable
    client = FakeClient.new('bad', 'bad', "[[ ## answer ## ]]\nok\n")
    Cosmos::Llm::Predict.configure { |settings| settings.max_parse_retries = 2 }

    prediction = Predict.new('question -> answer', client: client).call(question: 'Why?')

    assert_equal 'ok', prediction.answer
    assert_equal 3, client.call_count
  end

  def test_demos_appear_in_the_prompt
    client = FakeClient.new("[[ ## answer ## ]]\nyes\n")
    demo = Example.new(question: 'Prior?', answer: 'Prior answer')
    predictor = Predict.new('question -> answer', client: client, demos: [demo])

    predictor.call(question: 'Why?')

    contents = client.requests.first[:messages].map { |m| m[:content] }.join("\n")

    assert_includes contents, 'Prior answer'
  end

  def test_extra_params_reach_the_client
    client = FakeClient.new("[[ ## answer ## ]]\nyes\n")
    Predict.new('question -> answer', client: client, temperature: 0.3).call(question: 'Why?')

    assert_in_delta 0.3, client.requests.first[:temperature]
  end

  def test_settings_supply_model_and_max_tokens
    client = FakeClient.new("[[ ## answer ## ]]\nyes\n")
    Cosmos::Llm::Predict.configure do |settings|
      settings.model = 'test-model'
      settings.max_tokens = 256
    end

    Predict.new('question -> answer', client: client).call(question: 'Why?')

    assert_equal 'test-model', client.requests.first[:model]
    assert_equal 256, client.requests.first[:max_tokens]
  end

  def test_json_adapter_end_to_end
    client = FakeClient.new('{"answer": "yes"}')
    adapter = Cosmos::Llm::Predict::Adapters::JsonAdapter.new
    predictor = Predict.new('question -> answer', client: client, adapter: adapter)

    assert_equal 'yes', predictor.call(question: 'Why?').answer
  end

  def test_with_demos_leaves_the_receiver_alone
    predictor = Predict.new('question -> answer', client: FakeClient.new('x'))
    updated = predictor.with_demos([Example.new(question: 'a', answer: 'b')])

    assert_equal 1, updated.demos.length
    assert_empty predictor.demos
  end

  def test_with_instructions_leaves_the_receiver_alone
    predictor = Predict.new('question -> answer', client: FakeClient.new('x'))
    updated = predictor.with_instructions('Be terse.')

    assert_equal 'Be terse.', updated.signature.instructions
    refute_equal 'Be terse.', predictor.signature.instructions
  end

  def test_dump_and_load_state_round_trip
    predictor = Predict.new('question -> answer', client: FakeClient.new('x'))
    predictor.demos = [Example.new(question: 'a', answer: 'b')]

    state = predictor.dump_state
    restored = Predict.new('question -> answer', client: FakeClient.new('x'))
    restored.load_state(state)

    assert_equal 1, restored.demos.length
    assert_equal 'b', restored.demos.first[:answer]
  end

  def test_load_state_restores_instructions
    predictor = Predict.new('question -> answer', client: FakeClient.new('x'))
    predictor.load_state({ 'instructions' => 'Be terse.' })

    assert_equal 'Be terse.', predictor.signature.instructions
  end

  # Providers that predate the neutral #text method, and hand-rolled doubles,
  # expose choices -> message -> content instead.
  def test_reads_choices_message_content_responses
    choice = Struct.new(:message).new(Struct.new(:content).new("[[ ## answer ## ]]\nyes\n"))
    response = Struct.new(:choices).new([choice])
    client = Object.new
    client.define_singleton_method(:completion) { |**_params| response }

    assert_equal 'yes', Predict.new('question -> answer', client: client).call(question: 'Why?').answer
  end

  def test_raises_a_clear_error_for_an_unreadable_response
    client = Object.new
    client.define_singleton_method(:completion) { |**_params| Object.new }
    predictor = Predict.new('question -> answer', client: client)

    error = assert_raises(Cosmos::Llm::Predict::ConfigurationError) { predictor.call(question: 'Why?') }
    assert_match(/Cannot read text from/, error.message)
  end
end
