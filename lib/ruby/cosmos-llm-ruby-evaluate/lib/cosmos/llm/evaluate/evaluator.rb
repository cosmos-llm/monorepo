# frozen_string_literal: true

require_relative 'result'

module Cosmos
  module Llm
    module Evaluate
      # Runs a module over a devset and scores each result.
      #
      # Examples are independent, so they run concurrently up to +threads+. The
      # work is almost entirely waiting on HTTP, which is exactly the case where
      # Ruby threads help despite the GVL.
      #
      # A module that raises on one example does not abort the run: the failure
      # is recorded as a zero-scoring row and evaluation continues, up to
      # +max_errors+. Losing a whole devset sweep to one bad row is worse than
      # scoring it zero.
      #
      # @example
      #   evaluator = Evaluator.new(metric: Metrics.exact_match(:answer), threads: 8)
      #   result = evaluator.call(program, devset)
      #   puts result.summary
      class Evaluator
        # Default number of concurrent examples.
        DEFAULT_THREADS = 4

        # @return [#call] the metric being applied
        attr_reader :metric

        # @return [Integer] how many examples run concurrently
        attr_reader :threads

        # @return [Integer, nil] how many failures to tolerate
        attr_reader :max_errors

        # Builds an evaluator.
        #
        # @param metric [#call] receives (example, prediction) and returns a number
        # @param threads [Integer] how many examples to run concurrently
        # @param max_errors [Integer, nil] abort after this many failures; nil to
        #   tolerate any number
        # @param progress [#call, nil] called with (completed, total) after each example
        # @raise [ConfigurationError] if the metric is missing or threads is invalid
        def initialize(metric:, threads: DEFAULT_THREADS, max_errors: nil, progress: nil)
          raise ConfigurationError, 'Evaluator needs a metric responding to #call' unless metric.respond_to?(:call)

          unless threads.is_a?(Integer) && threads.positive?
            raise ConfigurationError, "Evaluator needs a positive thread count, got #{threads.inspect}"
          end

          @metric = metric
          @threads = threads
          @max_errors = max_errors
          @progress = progress
        end

        # Evaluates a module over a devset.
        #
        # @param program [#call] the module to evaluate
        # @param devset [Array<Cosmos::Llm::Predict::Example>] the examples
        # @return [Result] the scored result
        # @raise [TooManyErrorsError] if failures exceed +max_errors+
        # @example
        #   evaluator.call(program, devset)
        def call(program, devset)
          rows = Array.new(devset.length)
          queue = ::Queue.new
          devset.each_with_index { |example, index| queue << [example, index] }
          state = { completed: 0, failures: [] }
          mutex = ::Mutex.new

          workers = [@threads, devset.length].min
          pool = Array.new(workers) { worker(program, queue, rows, state, mutex) }
          pool.each(&:join)

          if @max_errors && state[:failures].length > @max_errors
            raise TooManyErrorsError.new(
              "Evaluation stopped after #{state[:failures].length} failures (max_errors: #{@max_errors}). " \
              "First failure: #{state[:failures].first[:error].message}",
              failures: state[:failures]
            )
          end

          Result.new(rows.compact)
        end
        alias evaluate call

        # Evaluates a module over a devset without constructing an evaluator.
        #
        # @param program [#call] the module to evaluate
        # @param devset [Array<Cosmos::Llm::Predict::Example>] the examples
        # @param metric [#call] the metric
        # @param options [Hash] remaining options passed to {#initialize}
        # @return [Result] the scored result
        # @example
        #   Evaluator.run(program, devset, metric: metric)
        def self.run(program, devset, metric:, **options)
          new(metric: metric, **options).call(program, devset)
        end

        private

        # Starts one worker thread draining the queue.
        #
        # @param program [#call] the module to evaluate
        # @param queue [Queue] pending (example, index) pairs
        # @param rows [Array] shared output, written by index
        # @param state [Hash] shared counters and failures
        # @param mutex [Mutex] guards +state+
        # @return [Thread] the worker
        def worker(program, queue, rows, state, mutex)
          ::Thread.new do
            loop do
              example, index = begin
                queue.pop(true)
              rescue ::ThreadError
                break
              end

              rows[index] = evaluate_one(program, example)
              mutex.synchronize do
                state[:failures] << { example: example, error: rows[index].error } if rows[index].error?
                state[:completed] += 1
                @progress&.call(state[:completed], rows.length)
              end
            end
          end
        end

        # Runs and scores a single example.
        #
        # A metric that raises is treated like a module that raises: the row
        # scores zero and carries the error, rather than taking down the run.
        #
        # @param program [#call] the module to evaluate
        # @param example [Cosmos::Llm::Predict::Example] the example
        # @return [Result::Row] the scored row
        def evaluate_one(program, example)
          prediction = program.call(**inputs_for(example))
          score = @metric.call(example, prediction).to_f
          Result::Row.new(example: example, prediction: prediction, score: score)
        rescue StandardError => e
          Result::Row.new(example: example, prediction: nil, score: 0.0, error: e)
        end

        # Extracts the module's arguments from an example.
        #
        # @param example [Cosmos::Llm::Predict::Example] the example
        # @return [Hash] the input values
        def inputs_for(example)
          return example.inputs if example.respond_to?(:inputs)
          return example.to_h if example.respond_to?(:to_h)

          example
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
