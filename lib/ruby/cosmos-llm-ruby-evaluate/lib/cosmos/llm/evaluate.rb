# frozen_string_literal: true

require 'cosmos/llm/predict'

require_relative 'evaluate/version'
require_relative 'evaluate/errors'
require_relative 'evaluate/evidence'
require_relative 'evaluate/metrics'
require_relative 'evaluate/result'
require_relative 'evaluate/evaluator'

module Cosmos
  module Llm
    # Scores a module against a labeled devset.
    #
    # An evaluation answers one question: over these examples, how often does
    # this program get it right? That number is what makes a change to a prompt
    # or a model an experiment rather than a guess, and it is the thing an
    # optimizer will eventually be maximizing.
    #
    # @example
    #   devset = [
    #     Cosmos::Llm::Predict::Example.new(question: 'What is 2+2?', answer: '4').with_inputs(:question)
    #   ]
    #
    #   result = Cosmos::Llm::Evaluate.run(
    #     program, devset,
    #     metric: Cosmos::Llm::Evaluate::Metrics.exact_match(:answer)
    #   )
    #
    #   puts result.summary          # => "100.0% (1/1 passed)"
    #   result.failures.each { |row| puts row.example[:question] }
    #
    # @see Evaluate::Evaluator
    # @see Evaluate::Metrics
    module Evaluate
      # Evaluates a module over a devset.
      #
      # @param program [#call] the module to evaluate
      # @param devset [Array<Cosmos::Llm::Predict::Example>] the examples
      # @param metric [#call] receives (example, prediction) and returns a number
      # @param options [Hash] remaining options passed to {Evaluator#initialize}
      # @return [Result] the scored result
      # @example
      #   Cosmos::Llm::Evaluate.run(program, devset, metric: metric, threads: 8)
      def self.run(program, devset, metric:, **options)
        Evaluator.run(program, devset, metric: metric, **options)
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
