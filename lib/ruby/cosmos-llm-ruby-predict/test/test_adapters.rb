# frozen_string_literal: true

require_relative 'test_helper'

class TestChatAdapter < Minitest::Test
  Adapter = Cosmos::Llm::Predict::Adapters::ChatAdapter
  Example = Cosmos::Llm::Predict::Example

  def setup
    @adapter = Adapter.new
    @signature = Cosmos::Llm::Signature.parse('question -> answer, confidence: number')
  end

  def test_system_prompt_includes_instructions_and_fields
    prompt = @adapter.system_prompt(@signature)

    assert_includes prompt, 'Given the fields `question`'
    assert_includes prompt, '- question (a string)'
    assert_includes prompt, '- confidence (a number)'
    assert_includes prompt, '[[ ## answer ## ]]'
  end

  def test_format_produces_system_then_user
    messages = @adapter.format(@signature, { question: 'Why?' })

    assert_equal %w[system user], messages.map { |m| m[:role] }
    assert_includes messages.last[:content], '[[ ## question ## ]]'
    assert_includes messages.last[:content], 'Why?'
  end

  def test_format_interleaves_demos
    demo = Example.new(question: 'Prior?', answer: 'Yes', confidence: 0.5)
    messages = @adapter.format(@signature, { question: 'Why?' }, [demo])

    assert_equal %w[system user assistant user], messages.map { |m| m[:role] }
    assert_includes messages[2][:content], 'Yes'
  end

  def test_extract_splits_sections
    completion = <<~TEXT
      [[ ## answer ## ]]
      Because of gravity.

      [[ ## confidence ## ]]
      0.8

      [[ ## done ## ]]
    TEXT

    result = @adapter.extract(@signature, completion)

    assert_equal 'Because of gravity.', result[:answer]
    assert_equal '0.8', result[:confidence]
  end

  def test_extract_preserves_multiline_values
    completion = "[[ ## answer ## ]]\nline one\nline two\n\n[[ ## confidence ## ]]\n1\n"

    result = @adapter.extract(@signature, completion)

    assert_equal "line one\nline two", result[:answer]
  end

  def test_extract_ignores_undeclared_sections
    completion = "[[ ## answer ## ]]\nyes\n\n[[ ## bogus ## ]]\nignored\n\n[[ ## confidence ## ]]\n1\n"

    result = @adapter.extract(@signature, completion)

    refute_includes result.keys, :bogus
  end

  def test_extract_raises_without_headers
    assert_raises(Cosmos::Llm::Predict::ParseError) { @adapter.extract(@signature, 'just prose') }
  end

  def test_parse_coerces_types
    completion = "[[ ## answer ## ]]\nyes\n\n[[ ## confidence ## ]]\n0.75\n"

    result = @adapter.parse(@signature, completion)

    assert_equal 'yes', result[:answer]
    assert_in_delta 0.75, result[:confidence]
  end

  def test_parse_raises_on_missing_required_field
    completion = "[[ ## answer ## ]]\nyes\n"

    error = assert_raises(Cosmos::Llm::Predict::ParseError) { @adapter.parse(@signature, completion) }
    assert_equal completion, error.completion
  end
end

class TestJsonAdapter < Minitest::Test
  Adapter = Cosmos::Llm::Predict::Adapters::JsonAdapter

  def setup
    @adapter = Adapter.new
    @signature = Cosmos::Llm::Signature.parse('question -> answer, confidence: number')
  end

  def test_format_instructions_lists_keys
    instructions = @adapter.format_instructions(@signature)

    assert_includes instructions, '"answer": a string'
    assert_includes instructions, '"confidence": a number'
  end

  def test_extract_plain_json
    result = @adapter.extract(@signature, '{"answer": "yes", "confidence": 0.9}')

    assert_equal 'yes', result[:answer]
    assert_in_delta 0.9, result[:confidence]
  end

  def test_extract_from_fenced_block
    completion = "Here you go:\n```json\n{\"answer\": \"yes\", \"confidence\": 1}\n```\n"

    result = @adapter.extract(@signature, completion)

    assert_equal 'yes', result[:answer]
  end

  def test_extract_from_surrounding_prose
    completion = 'Sure. {"answer": "yes", "confidence": 1} Hope that helps.'

    result = @adapter.extract(@signature, completion)

    assert_equal 'yes', result[:answer]
  end

  def test_extract_drops_undeclared_keys
    result = @adapter.extract(@signature, '{"answer": "yes", "confidence": 1, "extra": 2}')

    refute_includes result.keys, :extra
  end

  def test_extract_raises_without_json
    assert_raises(Cosmos::Llm::Predict::ParseError) { @adapter.extract(@signature, 'no json here') }
  end

  def test_request_params_empty_without_native_flag
    assert_empty @adapter.request_params(@signature)
  end

  def test_request_params_carry_schema_when_native
    adapter = Adapter.new(native_structured_output: true)

    params = adapter.request_params(@signature)

    assert_equal 'json_schema', params[:response_format][:type]
    assert_equal %w[answer confidence], params[:response_format][:json_schema][:schema]['properties'].keys
  end

  def test_render_inputs_emits_json
    rendered = @adapter.render_inputs(@signature, { question: 'Why?' })

    assert_equal({ 'question' => 'Why?' }, JSON.parse(rendered))
  end
end
