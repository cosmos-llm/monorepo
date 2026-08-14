# frozen_string_literal: true

module Cosmos
  module Llm
    module Tool
      # Reports the progress of a long-running agent run to a stream.
      #
      # An agent loop that thinks for two minutes between tool calls is
      # indistinguishable from a hung process. This gives a run somewhere to
      # say what it is doing without every caller inventing its own printf.
      #
      # Everything goes to stderr, so a CLI writing JSON to stdout stays
      # pipeable while still showing progress on a terminal.
      #
      # A live terminal gets a single line updated in place; a redirected
      # stream gets one line per update, since carriage returns are noise in a
      # log file.
      #
      # @example Report a loop's tool calls
      #   progress = Progress.new
      #   progress.step('researching')
      #   Loop.run(client: client, tools: registry, user: question,
      #            on_event: lambda { |kind, payload|
      #              progress.update("#{payload[:name]}...") if kind == :tool
      #            })
      #   progress.finish('done')
      class Progress
        # A Progress that reports nothing.
        #
        # Library code takes one of these by default so that a caller who wants
        # no output needs no conditionals at the call site.
        class Null
          # @return [void]
          def step(_message) = nil

          # @return [void]
          def update(_message) = nil

          # @return [void]
          def finish(_message = nil) = nil
        end

        # A Progress that discards all output.
        #
        # @return [Null] a no-op reporter
        def self.null = Null.new

        # @param io [IO] stream to write to
        # @param enabled [Boolean] false to suppress all output
        def initialize(io: $stderr, enabled: true)
          @io = io
          @enabled = enabled
          @tty = enabled && io.respond_to?(:tty?) && io.tty?
          @dirty = false
        end

        # Announces a new step on its own line.
        #
        # @param message [String] what is starting
        # @return [void]
        def step(message)
          return unless @enabled

          clear
          @io.puts(message)
          @io.flush
        end

        # Replaces the current status line, or logs it when not on a terminal.
        #
        # @param message [String] the current status
        # @return [void]
        def update(message)
          return unless @enabled

          if @tty
            @io.print("\r\e[K#{message}")
            @dirty = true
          else
            @io.puts(message)
          end
          @io.flush
        end

        # Ends the current status line.
        #
        # @param message [String, nil] a final status to leave on screen
        # @return [void]
        def finish(message = nil)
          return unless @enabled

          if message
            clear
            @io.puts(message)
          else
            clear
          end
          @io.flush
        end

        private

        # Erases a pending in-place status line, if any.
        #
        # @return [void]
        def clear
          return unless @dirty

          @io.print("\r\e[K")
          @dirty = false
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
