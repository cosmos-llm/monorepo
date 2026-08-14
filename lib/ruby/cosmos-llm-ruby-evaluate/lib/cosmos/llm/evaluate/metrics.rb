# frozen_string_literal: true

module Cosmos
  module Llm
    module Evaluate
      # Ready-made metric functions.
      #
      # A metric takes a labeled {Cosmos::Llm::Predict::Example} and a
      # {Cosmos::Llm::Predict::Prediction} and returns a number, usually in
      # 0.0..1.0. Anything responding to +call+ with that shape works, so these
      # are a convenience rather than a requirement.
      #
      # Each builder here returns a lambda bound to a field name, since a metric
      # has to know which field it is scoring.
      #
      # @example
      #   metric = Metrics.exact_match(:answer)
      #   metric.call(example, prediction)  # => 1.0 or 0.0
      module Metrics
        # Characters stripped from both sides before comparison.
        PUNCTUATION = /[[:punct:]]/.freeze

        # Words ignored when comparing token sets.
        STOP_WORDS = %w[a an the].freeze

        module_function

        # Scores 1.0 when the field matches the label exactly after normalization.
        #
        # Normalization lowercases, strips punctuation, collapses whitespace, and
        # drops leading articles — the differences that almost never matter when
        # checking whether a model got the answer right.
        #
        # @param field [Symbol] the field to compare
        # @param normalize [Boolean] whether to normalize before comparing
        # @return [Proc] a metric
        # @example
        #   Metrics.exact_match(:answer)
        def exact_match(field = :answer, normalize: true)
          lambda do |example, prediction|
            expected = value_of(example, field)
            actual = value_of(prediction, field)
            return 0.0 if expected.nil? || actual.nil?

            if normalize
              normalize_text(expected) == normalize_text(actual) ? 1.0 : 0.0
            else
              expected.to_s == actual.to_s ? 1.0 : 0.0
            end
          end
        end

        # Scores the token-level F1 between the field and its label.
        #
        # Useful where exact match is too strict — a correct answer phrased
        # differently, or one carrying extra words.
        #
        # @param field [Symbol] the field to compare
        # @return [Proc] a metric returning a score in 0.0..1.0
        # @example
        #   Metrics.f1(:answer)
        def f1(field = :answer)
          lambda do |example, prediction|
            expected = tokenize(value_of(example, field))
            actual = tokenize(value_of(prediction, field))
            next 0.0 if expected.empty? || actual.empty?

            overlap = count_overlap(expected, actual)
            next 0.0 if overlap.zero?

            precision = overlap.to_f / actual.length
            recall = overlap.to_f / expected.length
            (2 * precision * recall) / (precision + recall)
          end
        end

        # Scores 1.0 when the label appears anywhere in the field.
        #
        # @param field [Symbol] the field to search
        # @return [Proc] a metric
        # @example
        #   Metrics.contains(:answer)
        def contains(field = :answer)
          lambda do |example, prediction|
            expected = value_of(example, field)
            actual = value_of(prediction, field)
            next 0.0 if expected.nil? || actual.nil?

            normalize_text(actual).include?(normalize_text(expected)) ? 1.0 : 0.0
          end
        end

        # Scores the fraction of expected list items the prediction found.
        #
        # Order is ignored and duplicates are collapsed, which is what you want
        # for a set of extracted entities or tags.
        #
        # @param field [Symbol] the array field to compare
        # @return [Proc] a metric returning a score in 0.0..1.0
        def set_overlap(field = :answer)
          lambda do |example, prediction|
            expected = Array(value_of(example, field)).map { |item| normalize_text(item) }.uniq
            actual = Array(value_of(prediction, field)).map { |item| normalize_text(item) }.uniq
            next 0.0 if expected.empty?

            (expected & actual).length.to_f / expected.length
          end
        end

        # Scores the fraction of a prediction's quotes that appear in the source.
        #
        # This is the metric for extraction work: the prediction carries claims
        # with an +evidence+ quote attached to each, and the score is how many
        # of those quotes are actually in the document. A model that invents
        # support scores low even when its claims sound right, which is the
        # failure no answer-comparison metric detects.
        #
        # Unlike {contains}, comparison is a contiguous substring match after
        # Unicode-aware normalization, so a quote survives markdown conversion
        # and line rewrapping but a claim stitched together from scattered
        # words does not. See {Evidence} for what is and is not forgiven.
        #
        # A prediction carrying no quotes scores 1.0 by default: an extraction
        # that correctly found nothing has invented nothing. Pass
        # +empty_score: 0.0+ where a missing quote should count against it.
        #
        # @param field [Symbol] the prediction field holding the extracted
        #   structure to check
        # @param source [Symbol] the example field holding the source document
        # @param key [String] the field name holding quotes
        # @param empty_score [Float] the score when the prediction has no quotes
        # @return [Proc] a metric returning a score in 0.0..1.0
        # @example
        #   Metrics.grounded(:findings, source: :document)
        def grounded(field = :answer, source: :document, key: 'evidence', empty_score: 1.0)
          lambda do |example, prediction|
            text = value_of(example, source)
            extracted = value_of(prediction, field)
            next 0.0 if text.nil? || extracted.nil?

            quotes = Evidence.collect(extracted, key: key)
            next empty_score if quotes.empty?

            supported = quotes.count { |(_path, quote)| Evidence.present?(quote, text) }
            supported.to_f / quotes.length
          end
        end

        # Builds a metric that passes only when every supplied metric passes.
        #
        # @param metrics [Array<#call>] the metrics to combine
        # @param threshold [Float] the score each must reach
        # @return [Proc] a metric scoring 1.0 or 0.0
        # @example
        #   Metrics.all_of(Metrics.exact_match(:answer), Metrics.contains(:citation))
        def all_of(*metrics, threshold: 1.0)
          lambda do |example, prediction|
            metrics.all? { |metric| metric.call(example, prediction).to_f >= threshold } ? 1.0 : 0.0
          end
        end

        # Averages several metrics, optionally weighted.
        #
        # @param metrics [Array<#call>] the metrics to average
        # @param weights [Array<Numeric>, nil] one weight per metric
        # @return [Proc] a metric returning the weighted mean
        # @raise [ConfigurationError] if weights are given but do not match
        def average(*metrics, weights: nil)
          if weights && weights.length != metrics.length
            raise ConfigurationError,
                  "Got #{weights.length} weights for #{metrics.length} metrics"
          end

          effective = weights || Array.new(metrics.length, 1.0)
          total = effective.sum.to_f

          lambda do |example, prediction|
            next 0.0 if total.zero?

            metrics.each_with_index.sum { |metric, index| metric.call(example, prediction).to_f * effective[index] } / total
          end
        end

        # Reads a field from an example or prediction.
        #
        # @param record [#[]] an example or prediction
        # @param field [Symbol] the field name
        # @return [Object, nil] the value
        def value_of(record, field)
          return record[field] if record.respond_to?(:[])

          nil
        end

        # Lowercases, strips punctuation, drops articles, collapses whitespace.
        #
        # @param text [Object] the value to normalize
        # @return [String] the normalized text
        def normalize_text(text)
          words = text.to_s.downcase.gsub(PUNCTUATION, ' ').split
          stripped = words.reject { |word| STOP_WORDS.include?(word) }
          (stripped.empty? ? words : stripped).join(' ')
        end

        # @param text [Object] the value to tokenize
        # @return [Array<String>] normalized tokens
        def tokenize(text)
          normalize_text(text).split
        end

        # Counts shared tokens, respecting multiplicity.
        #
        # @param expected [Array<String>] the expected tokens
        # @param actual [Array<String>] the predicted tokens
        # @return [Integer] the number of overlapping tokens
        def count_overlap(expected, actual)
          remaining = actual.tally
          expected.count do |token|
            next false unless remaining[token].to_i.positive?

            remaining[token] -= 1
            true
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
