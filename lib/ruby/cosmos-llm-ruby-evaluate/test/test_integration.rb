# frozen_string_literal: true

require_relative 'test_helper'

# Exercises signature -> predict -> evaluate as one stack, with a fake client
# standing in for a provider.
class TestIntegration < Minitest::Test
  Example = Cosmos::Llm::Predict::Example
  Metrics = Cosmos::Llm::Evaluate::Metrics

  # Returns scripted completions keyed by a substring of the outgoing prompt.
  class RoutingClient
    Response = Struct.new(:text, :usage)

    attr_reader :requests

    def initialize(routes)
      @routes = routes
      @requests = []
      @mutex = Mutex.new
    end

    def completion(**params)
      @mutex.synchronize { @requests << params }
      prompt = params[:messages].map { |message| message[:content] }.join("\n")
      _, reply = @routes.find { |needle, _| prompt.include?(needle) }
      Response.new(reply.to_s, { 'input_tokens' => 5, 'output_tokens' => 3 })
    end
  end

  class Capitals < Cosmos::Llm::Signature
    instructions 'Name the capital city of the given country.'
    input :country, String, desc: 'a country name'
    output :capital, String, desc: 'the capital city'
    output :confidence, Float, desc: 'certainty between 0 and 1'
  end

  def reply(capital, confidence)
    "[[ ## capital ## ]]\n#{capital}\n\n[[ ## confidence ## ]]\n#{confidence}\n\n[[ ## done ## ]]\n"
  end

  def client
    RoutingClient.new(
      'France' => reply('Paris', '0.99'),
      'Japan' => reply('Tokyo', '0.98'),
      'Brazil' => reply('Rio de Janeiro', '0.60')
    )
  end

  def devset
    [
      Example.new(country: 'France', capital: 'Paris').with_inputs(:country),
      Example.new(country: 'Japan', capital: 'Tokyo').with_inputs(:country),
      Example.new(country: 'Brazil', capital: 'Brasilia').with_inputs(:country)
    ]
  end

  def test_full_stack_evaluation
    predictor = Cosmos::Llm::Predict::Predict.new(Capitals, client: client)

    result = Cosmos::Llm::Evaluate.run(predictor, devset, metric: Metrics.exact_match(:capital))

    assert_in_delta(2.0 / 3.0, result.score)
    assert_equal 'Brazil', result.failures.first.example[:country]
  end

  def test_predictions_carry_coerced_types
    predictor = Cosmos::Llm::Predict::Predict.new(Capitals, client: client)

    prediction = predictor.call(country: 'France')

    assert_equal 'Paris', prediction.capital
    assert_in_delta 0.99, prediction.confidence
    assert_kind_of Float, prediction.confidence
  end

  def test_signature_instructions_reach_the_prompt
    routing = client
    Cosmos::Llm::Predict::Predict.new(Capitals, client: routing).call(country: 'France')

    system_message = routing.requests.first[:messages].first

    assert_equal 'system', system_message[:role]
    assert_includes system_message[:content], 'Name the capital city'
    assert_includes system_message[:content], 'certainty between 0 and 1'
  end

  def test_cache_prevents_repeat_calls_across_an_evaluation
    routing = client
    cache = Cosmos::Llm::Predict::Cache.new
    predictor = Cosmos::Llm::Predict::Predict.new(Capitals, client: routing, cache: cache)

    Cosmos::Llm::Evaluate.run(predictor, devset, metric: Metrics.exact_match(:capital), threads: 1)
    Cosmos::Llm::Evaluate.run(predictor, devset, metric: Metrics.exact_match(:capital), threads: 1)

    assert_equal 3, routing.requests.length
  end

  def test_demonstrations_change_the_prompt_not_the_contract
    routing = client
    demo = Example.new(country: 'Italy', capital: 'Rome', confidence: 1.0)
    predictor = Cosmos::Llm::Predict::Predict.new(Capitals, client: routing, demos: [demo])

    prediction = predictor.call(country: 'France')

    roles = routing.requests.first[:messages].map { |message| message[:role] }

    assert_equal %w[system user assistant user], roles
    assert_equal 'Paris', prediction.capital
  end

  def test_json_adapter_produces_the_same_result
    json_client = RoutingClient.new(
      'France' => '{"capital": "Paris", "confidence": 0.99}',
      'Japan' => '{"capital": "Tokyo", "confidence": 0.98}',
      'Brazil' => '{"capital": "Rio de Janeiro", "confidence": 0.6}'
    )
    predictor = Cosmos::Llm::Predict::Predict.new(
      Capitals,
      client: json_client,
      adapter: Cosmos::Llm::Predict::Adapters::JsonAdapter.new
    )

    result = Cosmos::Llm::Evaluate.run(predictor, devset, metric: Metrics.exact_match(:capital))

    assert_in_delta(2.0 / 3.0, result.score)
  end

  def test_chain_of_thought_over_the_same_signature
    cot_client = RoutingClient.new(
      'France' => "[[ ## reasoning ## ]]\nFrance is in Europe.\n\n" \
                  "[[ ## capital ## ]]\nParis\n\n[[ ## confidence ## ]]\n1\n"
    )
    cot = Cosmos::Llm::Predict::ChainOfThought.new(Capitals, client: cot_client)

    prediction = cot.call(country: 'France')

    assert_equal 'France is in Europe.', prediction.reasoning
    assert_equal 'Paris', prediction.capital
  end

  def test_pipeline_of_modules_is_introspectable
    routing = client
    pipeline = Class.new(Cosmos::Llm::Predict::Module) do
      define_method(:initialize) do
        super()
        @lookup = Cosmos::Llm::Predict::Predict.new(Capitals, client: routing)
      end

      def forward(country:)
        @lookup.call(country: country)
      end
    end.new

    result = Cosmos::Llm::Evaluate.run(pipeline, devset, metric: Metrics.exact_match(:capital))

    assert_equal %w[lookup], pipeline.named_predictors.keys
    assert_in_delta(2.0 / 3.0, result.score)
  end
end
