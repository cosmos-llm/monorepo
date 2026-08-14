# frozen_string_literal: true

require 'test_helper'
require 'stringio'

module Cosmos
  module Llm
    module Tool
      class TestProgress < Minitest::Test
        # A StringIO that claims to be a terminal, to exercise the in-place path.
        class TtyIO < StringIO
          def tty? = true
        end

        def test_null_reporter_accepts_every_call_and_writes_nothing
          null = Progress.null

          assert_nil null.step('a')
          assert_nil null.update('b')
          assert_nil null.finish
          assert_nil null.finish('c')
        end

        def test_step_writes_one_line_per_call
          io = StringIO.new
          progress = Progress.new(io: io)

          progress.step('first')
          progress.step('second')

          assert_equal "first\nsecond\n", io.string
        end

        def test_non_tty_update_logs_a_line_rather_than_a_carriage_return
          io = StringIO.new
          progress = Progress.new(io: io)

          progress.update('working')

          assert_equal "working\n", io.string
          refute_includes io.string, "\r"
        end

        def test_tty_update_rewrites_in_place
          io = TtyIO.new
          progress = Progress.new(io: io)

          progress.update('50%')
          progress.update('60%')

          assert_equal "\r\e[K50%\r\e[K60%", io.string
        end

        def test_tty_step_clears_a_pending_status_line_first
          io = TtyIO.new
          progress = Progress.new(io: io)

          progress.update('working')
          progress.step('next phase')

          assert_equal "\r\e[Kworking\r\e[Knext phase\n", io.string
        end

        def test_finish_with_a_message_leaves_it_on_screen
          io = TtyIO.new
          progress = Progress.new(io: io)

          progress.update('working')
          progress.finish('complete')

          assert_equal "\r\e[Kworking\r\e[Kcomplete\n", io.string
        end

        def test_finish_without_a_message_just_clears
          io = TtyIO.new
          progress = Progress.new(io: io)

          progress.update('working')
          progress.finish

          assert_equal "\r\e[Kworking\r\e[K", io.string
        end

        def test_disabled_reporter_writes_nothing
          io = StringIO.new
          progress = Progress.new(io: io, enabled: false)

          progress.step('a')
          progress.update('b')
          progress.finish('c')

          assert_empty io.string
        end

        def test_clearing_is_not_repeated_when_nothing_is_pending
          io = TtyIO.new
          progress = Progress.new(io: io)

          progress.finish
          progress.finish

          assert_empty io.string
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
