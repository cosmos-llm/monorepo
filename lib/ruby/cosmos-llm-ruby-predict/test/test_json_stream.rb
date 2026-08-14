# frozen_string_literal: true

require 'test_helper'

class TestJsonStream < Minitest::Test
  JsonStream = Cosmos::Llm::Predict::JsonStream

  def test_splits_objects_separated_by_blank_lines
    text = <<~JSON
      {"value": 1}

      {"value": 2}
    JSON

    assert_equal ['{"value": 1}', '{"value": 2}'], JsonStream.split(text)
  end

  def test_splits_objects_run_together_on_one_line
    assert_equal ['{"a": 1}', '{"b": 2}'], JsonStream.split('{"a": 1}{"b": 2}')
  end

  def test_splits_objects_separated_by_commas
    assert_equal ['{"a": 1}', '{"b": 2}'], JsonStream.split('{"a": 1}, {"b": 2}')
  end

  def test_ignores_braces_inside_string_literals
    text = '{"evidence": "the config was {broken}"}'

    assert_equal [text], JsonStream.split(text)
  end

  def test_a_quote_spanning_a_paragraph_break_is_not_torn_in_half
    # The reason this cannot split on blank lines: a verbatim quote from a
    # source document routinely contains one. A raw newline inside a string is
    # not valid JSON, but the scan must still treat this as a single object so
    # that parse_all reports one broken object rather than two.
    text = %({"evidence": "first line\n\nsecond line"})

    assert_equal [text], JsonStream.split(text)
    assert_equal 1, JsonStream.parse_all(text)[:errors].length
  end

  def test_an_escaped_newline_inside_a_quote_parses_as_one_object
    # What a well-behaved model actually emits for the same quote.
    text = %({"evidence": "first line\\n\\nsecond line"})

    result = JsonStream.parse_all(text)

    assert_equal 1, result[:objects].length
    assert_equal "first line\n\nsecond line", result[:objects].first['evidence']
    assert_empty result[:errors]
  end

  def test_escaped_quotes_do_not_end_a_string
    text = '{"evidence": "he said \"hello\" then {left}"}'

    assert_equal [text], JsonStream.split(text)
  end

  def test_nested_objects_count_as_one
    text = '{"outer": {"inner": {"deep": 1}}}'

    assert_equal [text], JsonStream.split(text)
  end

  def test_prose_around_objects_is_skipped
    text = 'Here are the findings: {"a": 1} and also {"b": 2}. Done.'

    assert_equal ['{"a": 1}', '{"b": 2}'], JsonStream.split(text)
  end

  def test_stray_closing_brace_is_ignored
    assert_equal ['{"a": 1}'], JsonStream.split('} {"a": 1}')
  end

  def test_truncated_object_yields_nothing_from_the_split
    # Depth never returns to zero, so the incomplete object is not emitted.
    assert_empty JsonStream.split('{"a": 1')
  end

  def test_empty_input_yields_no_objects
    assert_empty JsonStream.split('')
    assert_empty JsonStream.split(nil)
  end

  # --- parse_all -------------------------------------------------------------

  def test_parse_all_returns_parsed_hashes
    result = JsonStream.parse_all(%({"value": 1}\n\n{"value": 2}))

    assert_equal [{ 'value' => 1 }, { 'value' => 2 }], result[:objects]
    assert_empty result[:errors]
  end

  def test_parse_all_keeps_good_objects_when_one_is_malformed
    # The middle object has a trailing comma. The findings around it survive.
    text = '{"a": 1} {"b": 2,} {"c": 3}'

    result = JsonStream.parse_all(text)

    assert_equal [{ 'a' => 1 }, { 'c' => 3 }], result[:objects]
    assert_equal 1, result[:errors].length
    assert_match(/object 1 did not parse/, result[:errors].first)
  end

  def test_parse_all_keeps_everything_before_a_truncation
    text = '{"a": 1} {"b": 2} {"c":'

    result = JsonStream.parse_all(text)

    assert_equal [{ 'a' => 1 }, { 'b' => 2 }], result[:objects]
    assert_empty result[:errors]
  end

  def test_parse_all_strips_a_surrounding_fence
    text = "```json\n{\"a\": 1}\n\n{\"b\": 2}\n```"

    result = JsonStream.parse_all(text)

    assert_equal [{ 'a' => 1 }, { 'b' => 2 }], result[:objects]
    assert_empty result[:errors]
  end

  def test_parse_all_handles_an_untagged_fence
    result = JsonStream.parse_all("```\n{\"a\": 1}\n```")

    assert_equal [{ 'a' => 1 }], result[:objects]
  end

  def test_strip_fences_leaves_unfenced_text_alone
    assert_equal '{"a": 1}', JsonStream.strip_fences('{"a": 1}')
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
