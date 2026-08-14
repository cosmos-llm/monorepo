# frozen_string_literal: true

require_relative 'test_helper'

class TestEvaluator < Minitest::Test
  Evaluator = Cosmos::Llm::Evaluate::Evaluator
  Metrics = Cosmos::Llm::Evaluate::Metrics
  Example = Cosmos::Llm::Predict::Example

  def devset
    [
      Example.new(question: 'a', answer: 'right').with_inputs(:question),
      Example.new(question: 'b', answer: 'right').with_inputs(:question),
      Example.new(question: 'c', answer: 'right').with_inputs(:question),
      Example.new(question: 'd', answer: 'right').with_inputs(:question)
    ]
  end

  def program(overrides = {})
    ScriptedProgram.new({ 'a' => 'right', 'b' => 'right', 'c' => 'wrong', 'd' => 'wrong' }.merge(overrides))
  end

  def metric
    Metrics.exact_match(:answer)
  end

  def test_scores_the_devset
    result = Evaluator.new(metric: metric).call(program, devset)

    assert_in_delta 0.5, result.score
    assert_in_delta 50.0, result.percent
    assert_equal 4, result.size
  end

  def test_calls_the_program_once_per_example
    prog = program
    Evaluator.new(metric: metric).call(prog, devset)

    assert_equal 4, prog.call_count
  end

  def test_passes_only_the_input_fields
    seen = nil
    prog = Class.new(Cosmos::Llm::Predict::Module) do
      define_method(:forward) do |**inputs|
        seen = inputs
        Cosmos::Llm::Predict::Prediction.new(answer: 'right')
      end
    end.new

    Evaluator.new(metric: metric).call(prog, devset.first(1))

    assert_equal({ question: 'a' }, seen)
  end

  def test_rows_stay_in_devset_order
    result = Evaluator.new(metric: metric, threads: 4).call(program, devset)

    assert_equal %w[a b c d], result.rows.map { |row| row.example[:question] }
  end

  def test_reports_passes_and_failures
    result = Evaluator.new(metric: metric).call(program, devset)

    assert_equal %w[a b], result.passes.map { |row| row.example[:question] }
    assert_equal %w[c d], result.failures.map { |row| row.example[:question] }
  end

  def test_records_an_error_row_without_aborting
    result = Evaluator.new(metric: metric).call(program('b' => RuntimeError.new('boom')), devset)

    assert_equal 1, result.errors.length
    assert_equal 4, result.size
    assert_in_delta 0.25, result.score
  end

  def test_error_rows_score_zero
    result = Evaluator.new(metric: metric).call(program('a' => RuntimeError.new('boom')), devset)
    row = result.rows.first

    assert row.error?
    assert_in_delta 0.0, row.score
    assert_nil row.prediction
  end

  def test_raises_when_errors_exceed_the_limit
    failing = program('a' => RuntimeError.new('boom'), 'b' => RuntimeError.new('boom'))

    error = assert_raises(Cosmos::Llm::Evaluate::TooManyErrorsError) do
      Evaluator.new(metric: metric, max_errors: 1).call(failing, devset)
    end
    assert_equal 2, error.failures.length
  end

  def test_tolerates_errors_within_the_limit
    failing = program('a' => RuntimeError.new('boom'))

    result = Evaluator.new(metric: metric, max_errors: 1).call(failing, devset)

    assert_equal 1, result.errors.length
  end

  def test_a_raising_metric_is_recorded_not_propagated
    exploding = ->(_example, _prediction) { raise 'metric failed' }

    result = Evaluator.new(metric: exploding).call(program, devset)

    assert_equal 4, result.errors.length
    assert_in_delta 0.0, result.score
  end

  def test_reports_progress
    seen = []
    Evaluator.new(metric: metric, threads: 1, progress: ->(done, total) { seen << [done, total] })
             .call(program, devset)

    assert_equal [[1, 4], [2, 4], [3, 4], [4, 4]], seen
  end

  def test_handles_an_empty_devset
    result = Evaluator.new(metric: metric).call(program, [])

    assert_in_delta 0.0, result.score
    assert_equal 0, result.size
  end

  def test_runs_concurrently_across_threads
    threads = Queue.new
    prog = Class.new(Cosmos::Llm::Predict::Module) do
      define_method(:forward) do |**_inputs|
        threads << Thread.current.object_id
        sleep 0.02
        Cosmos::Llm::Predict::Prediction.new(answer: 'right')
      end
    end.new

    Evaluator.new(metric: metric, threads: 4).call(prog, devset)

    assert_operator threads.size, :==, 4
    assert_operator Array.new(threads.size) { threads.pop }.uniq.length, :>, 1
  end

  def test_requires_a_callable_metric
    assert_raises(Cosmos::Llm::Evaluate::ConfigurationError) { Evaluator.new(metric: 'nope') }
  end

  def test_requires_a_positive_thread_count
    assert_raises(Cosmos::Llm::Evaluate::ConfigurationError) { Evaluator.new(metric: metric, threads: 0) }
  end

  def test_run_shorthand
    result = Evaluator.run(program, devset, metric: metric)

    assert_in_delta 0.5, result.score
  end

  def test_module_level_shorthand
    result = Cosmos::Llm::Evaluate.run(program, devset, metric: metric)

    assert_in_delta 0.5, result.score
  end

  def test_accepts_plain_hashes_as_examples
    plain = [{ question: 'a' }]
    result = Evaluator.new(metric: ->(_ex, pred) { pred[:answer] == 'right' ? 1.0 : 0.0 }).call(program, plain)

    assert_in_delta 1.0, result.score
  end
end

class TestResult < Minitest::Test
  Result = Cosmos::Llm::Evaluate::Result
  Example = Cosmos::Llm::Predict::Example

  def row(score, error: nil)
    Result::Row.new(example: Example.new(question: 'q'), prediction: nil, score: score, error: error)
  end

  def test_score_is_the_mean
    result = Result.new([row(1.0), row(0.0), row(0.5)])

    assert_in_delta 0.5, result.score
  end

  def test_failures_are_sorted_worst_first
    result = Result.new([row(0.5), row(0.0), row(1.0)])

    assert_equal [0.0, 0.5], result.failures.map(&:score)
  end

  def test_threshold_controls_passing
    result = Result.new([row(0.8), row(1.0)])

    assert_equal 1, result.passes.length
    assert_equal 2, result.passes(threshold: 0.5).length
  end

  def test_summary_reads_cleanly
    result = Result.new([row(1.0), row(0.0)])

    assert_equal '50.0% (1/2 passed)', result.summary
  end

  def test_summary_mentions_errors
    result = Result.new([row(1.0), row(0.0, error: RuntimeError.new('boom'))])

    assert_equal '50.0% (1/2 passed, 1 errored)', result.summary
  end

  def test_to_h_summarizes
    result = Result.new([row(1.0), row(0.0, error: RuntimeError.new('boom'))])

    assert_equal({ score: 0.5, size: 2, passed: 1, failed: 1, errored: 1 }, result.to_h)
  end
end
