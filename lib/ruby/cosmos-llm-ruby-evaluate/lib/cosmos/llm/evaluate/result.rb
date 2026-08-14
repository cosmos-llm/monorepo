# frozen_string_literal: true

module Cosmos
  module Llm
    module Evaluate
      # The outcome of evaluating a module over a devset.
      #
      # Holds the aggregate score and every per-example row behind it, because
      # the aggregate on its own rarely tells you what to fix. The rows are what
      # you sort to find the worst cases.
      #
      # @example
      #   result = Evaluator.new(metric: metric).call(program, devset)
      #   result.score          # => 0.72
      #   result.failures.first # => the lowest-scoring row
      class Result
        # One evaluated example: what went in, what came out, and what it scored.
        Row = Struct.new(:example, :prediction, :score, :error, keyword_init: true) do
          # @return [Boolean] whether the module raised on this example
          def error?
            !error.nil?
          end

          # @return [String] string representation
          def to_s
            error? ? "#<Row error=#{error.class}>" : "#<Row score=#{score}>"
          end
        end

        # @return [Array<Row>] one row per devset example, in devset order
        attr_reader :rows

        # Builds a result.
        #
        # @param rows [Array<Row>] the evaluated rows
        def initialize(rows)
          @rows = rows
        end

        # The mean score across all examples.
        #
        # Examples that raised count as zero rather than being dropped. A program
        # that crashes on a third of the devset has a real problem, and averaging
        # only over the survivors would hide it.
        #
        # @return [Float] the mean score, or 0.0 for an empty devset
        def score
          return 0.0 if @rows.empty?

          @rows.sum { |row| row.score.to_f } / @rows.length
        end

        # @return [Float] the mean score as a percentage
        def percent
          score * 100
        end

        # @return [Integer] how many examples were evaluated
        def size
          @rows.length
        end

        # @param threshold [Float] the passing score
        # @return [Array<Row>] rows at or above the threshold
        def passes(threshold: 1.0)
          @rows.select { |row| row.score.to_f >= threshold }
        end

        # Rows below the threshold, worst first.
        #
        # @param threshold [Float] the passing score
        # @return [Array<Row>] the failing rows, ascending by score
        def failures(threshold: 1.0)
          @rows.reject { |row| row.score.to_f >= threshold }.sort_by { |row| row.score.to_f }
        end

        # @return [Array<Row>] rows whose example raised
        def errors
          @rows.select(&:error?)
        end

        # @return [Hash] a summary suitable for logging
        def to_h
          {
            score: score,
            size: size,
            passed: passes.length,
            failed: failures.length,
            errored: errors.length
          }
        end

        # A short human-readable summary.
        #
        # @return [String] e.g. "72.0% (36/50 passed, 2 errored)"
        def summary
          detail = "#{passes.length}/#{size} passed"
          detail += ", #{errors.length} errored" unless errors.empty?
          "#{format('%.1f', percent)}% (#{detail})"
        end

        # @return [String] string representation
        def to_s
          "#<Evaluate::Result #{summary}>"
        end
        alias inspect to_s
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
