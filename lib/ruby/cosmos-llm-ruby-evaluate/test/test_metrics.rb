# frozen_string_literal: true

require_relative 'test_helper'

class TestMetrics < Minitest::Test
  Metrics = Cosmos::Llm::Evaluate::Metrics
  Example = Cosmos::Llm::Predict::Example
  Prediction = Cosmos::Llm::Predict::Prediction

  def pair(expected, actual, field: :answer)
    [Example.new(field => expected), Prediction.new(field => actual)]
  end

  def test_exact_match_scores_identical_values
    metric = Metrics.exact_match(:answer)

    assert_in_delta 1.0, metric.call(*pair('Paris', 'Paris'))
  end

  def test_exact_match_ignores_case_and_punctuation
    metric = Metrics.exact_match(:answer)

    assert_in_delta 1.0, metric.call(*pair('Paris', 'paris.'))
  end

  def test_exact_match_ignores_leading_articles
    metric = Metrics.exact_match(:answer)

    assert_in_delta 1.0, metric.call(*pair('the Eiffel Tower', 'Eiffel Tower'))
  end

  def test_exact_match_rejects_different_values
    metric = Metrics.exact_match(:answer)

    assert_in_delta 0.0, metric.call(*pair('Paris', 'London'))
  end

  def test_exact_match_can_be_strict
    metric = Metrics.exact_match(:answer, normalize: false)

    assert_in_delta 0.0, metric.call(*pair('Paris', 'paris'))
  end

  def test_exact_match_scores_zero_on_missing_field
    metric = Metrics.exact_match(:answer)

    assert_in_delta 0.0, metric.call(Example.new(answer: 'Paris'), Prediction.new(other: 'Paris'))
  end

  def test_f1_scores_full_overlap
    metric = Metrics.f1(:answer)

    assert_in_delta 1.0, metric.call(*pair('the quick fox', 'quick fox'))
  end

  def test_f1_scores_partial_overlap
    metric = Metrics.f1(:answer)
    score = metric.call(*pair('quick brown fox', 'quick fox'))

    assert_operator score, :>, 0.0
    assert_operator score, :<, 1.0
  end

  def test_f1_scores_zero_without_overlap
    metric = Metrics.f1(:answer)

    assert_in_delta 0.0, metric.call(*pair('cats', 'dogs'))
  end

  def test_f1_handles_repeated_tokens
    metric = Metrics.f1(:answer)
    score = metric.call(*pair('yes yes', 'yes'))

    assert_operator score, :>, 0.0
    assert_operator score, :<, 1.0
  end

  def test_contains_finds_a_substring
    metric = Metrics.contains(:answer)

    assert_in_delta 1.0, metric.call(*pair('Paris', 'The capital is Paris.'))
  end

  def test_contains_rejects_absent_text
    metric = Metrics.contains(:answer)

    assert_in_delta 0.0, metric.call(*pair('Paris', 'The capital is London.'))
  end

  def test_set_overlap_scores_the_found_fraction
    metric = Metrics.set_overlap(:answer)

    assert_in_delta 0.5, metric.call(*pair(%w[a b], %w[a c]))
  end

  def test_set_overlap_ignores_order_and_duplicates
    metric = Metrics.set_overlap(:answer)

    assert_in_delta 1.0, metric.call(*pair(%w[a b], %w[b a a]))
  end

  def test_set_overlap_scores_zero_for_empty_expectation
    metric = Metrics.set_overlap(:answer)

    assert_in_delta 0.0, metric.call(*pair([], %w[a]))
  end

  def test_all_of_requires_every_metric
    metric = Metrics.all_of(Metrics.exact_match(:answer), Metrics.contains(:answer))

    assert_in_delta 1.0, metric.call(*pair('Paris', 'Paris'))
    assert_in_delta 0.0, metric.call(*pair('Paris', 'London'))
  end

  def test_average_combines_metrics
    metric = Metrics.average(Metrics.exact_match(:answer), Metrics.contains(:answer))

    assert_in_delta 0.5, metric.call(*pair('Paris', 'Paris is the capital'))
  end

  def test_average_respects_weights
    metric = Metrics.average(
      Metrics.exact_match(:answer),
      Metrics.contains(:answer),
      weights: [3.0, 1.0]
    )

    assert_in_delta 0.25, metric.call(*pair('Paris', 'Paris is the capital'))
  end

  def test_average_rejects_mismatched_weights
    assert_raises(Cosmos::Llm::Evaluate::ConfigurationError) do
      Metrics.average(Metrics.exact_match(:answer), weights: [1.0, 2.0])
    end
  end
end
