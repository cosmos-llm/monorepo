# frozen_string_literal: true

require 'test_helper'

module Cosmos
  module Llm
    module Tool
      class TestSession < Minitest::Test
        def test_unmetered_key_is_always_allowed
          session = Session.new

          20.times { session.consume(:anything) }

          assert session.allowed?(:anything)
          assert_nil session.remaining(:anything)
          assert_equal 20, session.counts[:anything]
        end

        def test_metered_key_is_refused_once_the_cap_is_reached
          session = Session.new(budgets: { search: 2 })

          assert session.allowed?(:search)
          session.consume(:search)
          assert session.allowed?(:search)
          session.consume(:search)

          refute session.allowed?(:search)
        end

        def test_remaining_counts_down_and_floors_at_zero
          session = Session.new(budgets: { open: 2 })

          assert_equal 2, session.remaining(:open)
          session.consume(:open)
          assert_equal 1, session.remaining(:open)
          session.consume(:open, 5)
          assert_equal 0, session.remaining(:open)
        end

        def test_counts_continue_past_the_cap
          session = Session.new(budgets: { search: 1 })

          3.times { session.consume(:search) }

          assert_equal 3, session.counts[:search]
          assert_equal 0, session.remaining(:search)
        end

        def test_string_and_symbol_keys_are_the_same_budget
          session = Session.new(budgets: { 'search' => 1 })

          session.consume('search')

          refute session.allowed?(:search)
        end

        def test_exhausted_message_names_the_key_and_the_count
          session = Session.new(budgets: { search: 1 })
          session.consume(:search)

          message = session.exhausted_message(:search)

          assert_match(/search budget exhausted/, message)
          assert_match(/1 used/, message)
        end

        def test_finish_sets_the_flag_and_records_a_note
          session = Session.new

          refute_predicate session, :finished?
          session.finish!('queued 4 documents')

          assert_predicate session, :finished?
          assert_equal ['queued 4 documents'], session.notes
        end

        def test_finish_without_a_note_records_nothing
          session = Session.new
          session.finish!

          assert_predicate session, :finished?
          assert_empty session.notes
        end

        def test_to_h_summarizes_the_run
          session = Session.new(budgets: { search: 5 })
          session.consume(:search, 2)
          session.note('halfway')

          summary = session.to_h

          assert_equal({ search: 2 }, summary[:counts])
          assert_equal({ search: 5 }, summary[:budgets])
          assert_equal ['halfway'], summary[:notes]
          refute summary[:finished]
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
