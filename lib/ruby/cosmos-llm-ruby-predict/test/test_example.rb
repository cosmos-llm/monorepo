# frozen_string_literal: true

require_relative 'test_helper'

class TestExample < Minitest::Test
  Example = Cosmos::Llm::Predict::Example

  def test_reads_values_by_key_and_method
    ex = Example.new(question: 'Why?', answer: 'Because.')

    assert_equal 'Why?', ex[:question]
    assert_equal 'Why?', ex['question']
    assert_equal 'Because.', ex.answer
  end

  def test_unknown_method_still_raises
    ex = Example.new(question: 'Why?')

    assert_raises(NoMethodError) { ex.nonexistent }
  end

  def test_responds_to_declared_fields
    ex = Example.new(question: 'Why?')

    assert_respond_to ex, :question
    refute_respond_to ex, :answer
  end

  def test_all_values_are_inputs_when_unmarked
    ex = Example.new(question: 'Why?', answer: 'Because.')

    assert_equal({ question: 'Why?', answer: 'Because.' }, ex.inputs)
    assert_empty ex.labels
  end

  def test_with_inputs_splits_inputs_from_labels
    ex = Example.new(question: 'Why?', answer: 'Because.').with_inputs(:question)

    assert_equal({ question: 'Why?' }, ex.inputs)
    assert_equal({ answer: 'Because.' }, ex.labels)
  end

  def test_with_inputs_does_not_mutate_the_receiver
    original = Example.new(question: 'Why?', answer: 'Because.')
    original.with_inputs(:question)

    assert_empty original.input_keys
  end

  def test_merge_returns_a_new_example
    ex = Example.new(question: 'Why?').with_inputs(:question)
    merged = ex.merge(answer: 'Because.')

    assert_equal 'Because.', merged[:answer]
    assert_equal %i[question], merged.input_keys
    refute ex.key?(:answer)
  end

  def test_except_drops_keys_and_input_marks
    ex = Example.new(question: 'Why?', answer: 'Because.').with_inputs(:question)
    trimmed = ex.except(:question)

    refute trimmed.key?(:question)
    assert_empty trimmed.input_keys
  end

  def test_normalizes_string_keys
    ex = Example.new('question' => 'Why?')

    assert_equal 'Why?', ex[:question]
  end

  def test_equality_compares_values_and_input_keys
    a = Example.new(question: 'Why?').with_inputs(:question)
    b = Example.new(question: 'Why?').with_inputs(:question)
    c = Example.new(question: 'Why?')

    assert_equal a, b
    refute_equal a, c
  end

  def test_is_enumerable
    ex = Example.new(a: 1, b: 2)

    assert_equal [%i[a].first, 1], ex.first
    assert_equal 3, ex.sum { |_name, value| value }
  end
end
