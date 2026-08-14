# frozen_string_literal: true

require 'test_helper'

class TestEvidence < Minitest::Test
  Evidence = Cosmos::Llm::Evaluate::Evidence

  SOURCE = <<~TEXT
    They saw their conversion rate increase by 7-12% after the migration.

    The client came to us with a checkout flow that took eleven seconds
    to render on a cold cache.
  TEXT

  def test_verbatim_quote_is_present
    assert Evidence.present?('conversion rate increase by 7-12%', SOURCE)
  end

  def test_absent_quote_is_not_present
    refute Evidence.present?('conversion rate fell by 40%', SOURCE)
  end

  def test_leading_word_lowercased_by_the_model_still_verifies
    # A model copying from mid-paragraph routinely lowercases the first word.
    assert Evidence.present?('they saw their conversion rate', SOURCE)
  end

  def test_quote_spanning_a_line_break_verifies
    # The source wraps between "seconds" and "to render".
    assert Evidence.present?('took eleven seconds to render on a cold cache', SOURCE)
  end

  def test_curly_quotes_and_em_dashes_are_folded
    source = 'The vendor said “it doubled” — every quarter.'

    assert Evidence.present?('"it doubled" - every quarter', source)
  end

  def test_scattered_words_do_not_verify
    # Every word appears in the source, but not contiguously. This is the
    # invention the check exists to catch.
    refute Evidence.present?('conversion rate cold cache eleven', SOURCE)
  end

  def test_empty_quote_is_never_present
    refute Evidence.present?('', SOURCE)
    refute Evidence.present?(nil, SOURCE)
    refute Evidence.present?('   ', SOURCE)
  end

  def test_normalize_collapses_whitespace_and_folds_punctuation
    assert_equal "a 'b' \"c\" - d", Evidence.normalize("a  ‘b’\n “c”  — d")
  end

  # --- collect ---------------------------------------------------------------

  def test_collect_finds_nested_quotes_with_paths
    record = {
      'findings' => [
        { 'value' => '7%', 'evidence' => 'first quote' },
        { 'value' => '9%', 'evidence' => 'second quote' }
      ]
    }

    assert_equal(
      [['findings[0].evidence', 'first quote'], ['findings[1].evidence', 'second quote']],
      Evidence.collect(record)
    )
  end

  def test_collect_matches_symbol_keys
    assert_equal [['evidence', 'a quote']], Evidence.collect({ evidence: 'a quote' })
  end

  def test_collect_ignores_a_non_string_evidence_value
    assert_empty Evidence.collect({ 'evidence' => { 'nested' => 1 } })
  end

  def test_collect_honors_a_custom_key
    record = { 'quote' => 'text here' }

    assert_equal [['quote', 'text here']], Evidence.collect(record, key: 'quote')
    assert_empty Evidence.collect(record)
  end

  # --- unverified ------------------------------------------------------------

  def test_unverified_reports_only_the_unsupported_quotes
    record = {
      'findings' => [
        { 'evidence' => 'conversion rate increase by 7-12%' },
        { 'evidence' => 'revenue tripled overnight' }
      ]
    }

    messages = Evidence.unverified(record, SOURCE)

    assert_equal 1, messages.length
    assert_match(/findings\[1\]\.evidence/, messages.first)
    assert_match(/revenue tripled overnight/, messages.first)
  end

  def test_unverified_is_empty_when_everything_checks_out
    record = { 'evidence' => 'the client came to us' }

    assert_empty Evidence.unverified(record, SOURCE)
    assert Evidence.verified?(record, SOURCE)
  end

  def test_unverified_truncates_a_long_quote_in_the_message
    record = { 'evidence' => "#{'x' * 200} never appears" }

    message = Evidence.unverified(record, SOURCE).first

    assert_operator message.length, :<, 140
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
