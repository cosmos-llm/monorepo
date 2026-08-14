# frozen_string_literal: true

require_relative 'test_helper'

class TestChainOfThought < Minitest::Test
  ChainOfThought = Cosmos::Llm::Predict::ChainOfThought

  def test_injects_a_reasoning_field_before_the_outputs
    cot = ChainOfThought.new('question -> answer')

    assert_equal %i[reasoning answer], cot.signature.output_fields.keys
  end

  def test_preserves_inputs_and_instructions
    base = Cosmos::Llm::Signature.parse('question, context -> answer').with_instructions('Be careful.')
    cot = ChainOfThought.new(base)

    assert_equal %i[question context], cot.signature.input_fields.keys
    assert_equal 'Be careful.', cot.signature.instructions
  end

  def test_preserves_output_field_types
    cot = ChainOfThought.new('question -> score: number, tags: array[string]')

    assert_equal :number, cot.signature.output_fields[:score].type
    assert_equal :string, cot.signature.output_fields[:tags].element_type
  end

  def test_reasoning_is_available_on_the_prediction
    completion = "[[ ## reasoning ## ]]\nTwo plus two.\n\n[[ ## answer ## ]]\nfour\n"
    cot = ChainOfThought.new('question -> answer', client: FakeClient.new(completion))

    prediction = cot.call(question: 'What is 2+2?')

    assert_equal 'Two plus two.', prediction.reasoning
    assert_equal 'four', prediction.answer
  end

  def test_rejects_a_colliding_field_name
    assert_raises(Cosmos::Llm::Signature::DefinitionError) do
      ChainOfThought.new('question -> reasoning, answer')
    end
  end

  def test_custom_reasoning_field_name_avoids_collisions
    cot = ChainOfThought.new('question -> reasoning, answer', reasoning_field: :scratchpad)

    assert_equal %i[scratchpad reasoning answer], cot.signature.output_fields.keys
  end
end

class TestBestOfN < Minitest::Test
  BestOfN = Cosmos::Llm::Predict::BestOfN
  Predict = Cosmos::Llm::Predict::Predict

  def reply(answer)
    "[[ ## answer ## ]]\n#{answer}\n"
  end

  def test_picks_the_highest_scoring_attempt
    client = FakeClient.new(reply('a'), reply('bbb'), reply('cc'))
    predictor = Predict.new('question -> answer', client: client)
    sampler = BestOfN.new(predictor, n: 3) { |_inputs, pred| pred.answer.length }

    prediction = sampler.call(question: 'Why?')

    assert_equal 'bbb', prediction.answer
    assert_equal 3, client.call_count
  end

  def test_records_score_and_attempt_count
    client = FakeClient.new(reply('a'), reply('bb'))
    sampler = BestOfN.new(Predict.new('question -> answer', client: client), n: 2) do |_inputs, pred|
      pred.answer.length
    end

    prediction = sampler.call(question: 'Why?')

    assert_in_delta 2.0, prediction[:best_of_n_score]
    assert_equal 2, prediction[:best_of_n_attempts]
  end

  def test_threshold_stops_early
    client = FakeClient.new(reply('good'), reply('unused'))
    sampler = BestOfN.new(Predict.new('question -> answer', client: client), n: 3, threshold: 1.0) do |_inputs, pred|
      pred.answer == 'good' ? 1.0 : 0.0
    end

    prediction = sampler.call(question: 'Why?')

    assert_equal 'good', prediction.answer
    assert_equal 1, client.call_count
  end

  def test_survives_a_failing_attempt
    client = FakeClient.new('malformed', reply('recovered'))
    Cosmos::Llm::Predict.configure { |settings| settings.max_parse_retries = 0 }
    sampler = BestOfN.new(Predict.new('question -> answer', client: client), n: 2) { |_inputs, _pred| 1.0 }

    assert_equal 'recovered', sampler.call(question: 'Why?').answer
  end

  def test_raises_when_every_attempt_fails
    client = FakeClient.new('malformed')
    Cosmos::Llm::Predict.configure { |settings| settings.max_parse_retries = 0 }
    sampler = BestOfN.new(Predict.new('question -> answer', client: client), n: 2) { |_inputs, _pred| 1.0 }

    assert_raises(Cosmos::Llm::Predict::ParseError) { sampler.call(question: 'Why?') }
  end

  def test_samples_at_the_given_temperature
    client = FakeClient.new(reply('a'))
    sampler = BestOfN.new(Predict.new('question -> answer', client: client), n: 1, temperature: 1.5) do
      1.0
    end

    sampler.call(question: 'Why?')

    assert_in_delta 1.5, client.requests.first[:temperature]
  end

  def test_restores_settings_after_sampling
    client = FakeClient.new(reply('a'))
    sampler = BestOfN.new(Predict.new('question -> answer', client: client), n: 1, temperature: 1.5) { 1.0 }

    sampler.call(question: 'Why?')

    assert_nil Cosmos::Llm::Predict.settings.temperature
  end

  def test_requires_a_reward_function
    assert_raises(Cosmos::Llm::Predict::ConfigurationError) do
      BestOfN.new(Predict.new('question -> answer'), n: 2)
    end
  end

  def test_requires_a_positive_n
    assert_raises(Cosmos::Llm::Predict::ConfigurationError) do
      BestOfN.new(Predict.new('question -> answer'), n: 0) { 1.0 }
    end
  end
end

class TestModuleIntrospection < Minitest::Test
  Predict = Cosmos::Llm::Predict::Predict

  class Pipeline < Cosmos::Llm::Predict::Module
    attr_reader :draft, :critique

    def initialize
      super
      @draft = Predict.new('question -> answer')
      @critique = Predict.new('answer -> verdict')
      @label = 'not a predictor'
    end

    def forward(question:)
      @draft.call(question: question)
    end
  end

  class NestedPipeline < Cosmos::Llm::Predict::Module
    def initialize
      super
      @inner = Pipeline.new
      @extras = [Predict.new('a -> b')]
    end

    def forward(**inputs)
      @inner.call(**inputs)
    end
  end

  def test_finds_nested_predictors
    assert_equal %w[draft critique], Pipeline.new.named_predictors.keys
  end

  def test_walks_into_sub_modules_and_arrays
    paths = NestedPipeline.new.named_predictors.keys

    assert_includes paths, 'inner.draft'
    assert_includes paths, 'extras[0]'
  end

  def test_predictors_returns_the_instances
    assert_equal 2, Pipeline.new.predictors.length
  end

  def test_dump_and_load_state_round_trip
    pipeline = Pipeline.new
    pipeline.draft.demos = [Cosmos::Llm::Predict::Example.new(question: 'a', answer: 'b')]

    state = pipeline.dump_state
    restored = Pipeline.new.load_state(state)

    assert_equal 1, restored.draft.demos.length
  end

  def test_load_state_rejects_unknown_paths
    assert_raises(Cosmos::Llm::Predict::ConfigurationError) do
      Pipeline.new.load_state({ 'nonexistent' => { 'demos' => [] } })
    end
  end

  def test_deep_copy_is_independent
    pipeline = Pipeline.new
    copy = pipeline.deep_copy
    copy.draft.demos = [Cosmos::Llm::Predict::Example.new(question: 'a', answer: 'b')]

    assert_empty pipeline.draft.demos
  end

  def test_forward_must_be_implemented
    assert_raises(NotImplementedError) { Cosmos::Llm::Predict::Module.new.call }
  end
end
