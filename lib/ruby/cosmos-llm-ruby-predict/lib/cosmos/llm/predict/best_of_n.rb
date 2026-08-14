# frozen_string_literal: true

require_relative 'module'
require_relative 'settings'

module Cosmos
  module Llm
    module Predict
      # Samples a module several times and keeps the best-scoring result.
      #
      # Sampling is done at a raised temperature so the attempts actually differ,
      # and each is scored by a reward function. The first attempt at or above
      # +threshold+ short-circuits the rest, so an easy input costs one call and
      # only a hard one pays for all N.
      #
      # A reward function receives the inputs and a prediction and returns a
      # number. What it measures is entirely the caller's business: a checksum, a
      # validator, another LLM call.
      #
      # @example Keep the longest of three answers
      #   sampler = BestOfN.new(predictor, n: 3) { |_inputs, pred| pred.answer.length }
      #   sampler.call(question: 'Explain TCP slow start.')
      #
      # @example Stop as soon as an answer validates
      #   BestOfN.new(predictor, n: 5, threshold: 1.0) do |_inputs, pred|
      #     valid_json?(pred.answer) ? 1.0 : 0.0
      #   end
      class BestOfN < Module
        # @return [Module] the module being sampled
        attr_reader :module

        # @return [Integer] how many attempts to make
        attr_reader :n

        # @return [Float, nil] score at which to stop early
        attr_reader :threshold

        # @return [Float] temperature used for sampling
        attr_reader :temperature

        # Builds a sampler.
        #
        # @param mod [Module] the module to sample
        # @param n [Integer] how many attempts to make
        # @param threshold [Float, nil] stop at the first score meeting this
        # @param temperature [Float] sampling temperature for the attempts
        # @param reward [#call, nil] the reward function, if not given as a block
        # @yield [inputs, prediction] alternative way to supply the reward function
        # @raise [ConfigurationError] if n is not positive or no reward is given
        def initialize(mod, n: 3, threshold: nil, temperature: 1.0, reward: nil, &block)
          raise ConfigurationError, "BestOfN needs a positive n, got #{n.inspect}" unless n.is_a?(Integer) && n.positive?

          @reward = reward || block
          raise ConfigurationError, 'BestOfN needs a reward function, given as reward: or a block' unless @reward

          @module = mod
          @n = n
          @threshold = threshold
          @temperature = temperature
        end

        # Samples the module and returns the best prediction.
        #
        # Attempts that raise are skipped rather than aborting the run; with N
        # samples in hand there is no reason to let one bad draw lose the others.
        # If every attempt fails, the last error is re-raised.
        #
        # @param inputs [Hash] the input field values
        # @return [Prediction] the best-scoring prediction, with +best_of_n_score+
        #   and +best_of_n_attempts+ merged in
        # @raise [StandardError] if every attempt raised
        def forward(**inputs)
          best = nil
          best_score = nil
          attempts = 0
          last_error = nil

          @n.times do
            attempts += 1
            begin
              prediction = sample(inputs)
            rescue StandardError => e
              last_error = e
              next
            end

            score = @reward.call(inputs, prediction).to_f
            if best_score.nil? || score > best_score
              best = prediction
              best_score = score
            end
            break if @threshold && score >= @threshold
          end

          raise last_error if best.nil? && last_error
          raise ConfigurationError, 'BestOfN produced no predictions' if best.nil?

          best.merge(best_of_n_score: best_score, best_of_n_attempts: attempts)
        end

        private

        # Runs one attempt at the sampler's temperature.
        #
        # @param inputs [Hash] the input field values
        # @return [Prediction] the attempt's result
        def sample(inputs)
          Cosmos::Llm::Predict.with(temperature: @temperature) do
            @module.call(**inputs)
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
