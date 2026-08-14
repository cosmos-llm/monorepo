# frozen_string_literal: true

module Cosmos
  module Llm
    module Predict
      # A record of field values, some of which are designated as inputs.
      #
      # An Example is the unit of data everywhere else in the stack: a training
      # or evaluation record, a few-shot demonstration, a set of arguments to a
      # module. It is a plain bag of named values plus a marker for which keys
      # are inputs, and that marker is what lets the same object serve as both
      # "the question" and "the question with its known answer".
      #
      # @example A labeled record for evaluation
      #   ex = Example.new(question: 'What is 2+2?', answer: '4').with_inputs(:question)
      #   ex.inputs  # => { question: 'What is 2+2?' }
      #   ex.labels  # => { answer: '4' }
      #
      # @example Reading values
      #   ex[:answer]  # => '4'
      #   ex.answer    # => '4'
      class Example
        include Enumerable

        # @return [Hash{Symbol => Object}] all field values
        attr_reader :values

        # @return [Array<Symbol>] keys designated as inputs
        attr_reader :input_keys

        # Builds an example.
        #
        # @param values [Hash] the field values; keys may be strings or symbols
        # @example
        #   Example.new(question: 'Why?', answer: 'Because.')
        def initialize(**values)
          @values = normalize(values)
          @input_keys = []
        end

        # Returns a copy with the given keys designated as inputs.
        #
        # @param keys [Array<Symbol, String>] the input field names
        # @return [Example] a new example; the receiver is unchanged
        # @example
        #   ex.with_inputs(:question, :context)
        def with_inputs(*keys)
          copy = dup_with(@values)
          copy.input_keys = keys.flatten.map { |key| key.to_s.to_sym }
          copy
        end

        # Returns a copy with additional or replaced values.
        #
        # @param updates [Hash] values to merge in
        # @return [Example] a new example; the receiver is unchanged
        # @example
        #   ex.merge(answer: '4')
        def merge(**updates)
          copy = dup_with(@values.merge(normalize(updates)))
          copy.input_keys = @input_keys
          copy
        end

        # Returns a copy without the given keys.
        #
        # @param keys [Array<Symbol, String>] field names to drop
        # @return [Example] a new example; the receiver is unchanged
        def except(*keys)
          dropped = keys.flatten.map { |key| key.to_s.to_sym }
          copy = dup_with(@values.reject { |key, _| dropped.include?(key) })
          copy.input_keys = @input_keys - dropped
          copy
        end

        # The values designated as inputs.
        #
        # When no input keys have been set, every value counts as an input; an
        # unlabeled example is all question and no answer.
        #
        # @return [Hash{Symbol => Object}] the input values
        def inputs
          return @values.dup if @input_keys.empty?

          @values.select { |key, _| @input_keys.include?(key) }
        end

        # The values not designated as inputs.
        #
        # @return [Hash{Symbol => Object}] the label values
        def labels
          return {} if @input_keys.empty?

          @values.reject { |key, _| @input_keys.include?(key) }
        end

        # @param key [Symbol, String] a field name
        # @return [Object, nil] the value, or nil if absent
        def [](key)
          @values[key.to_s.to_sym]
        end

        # @param key [Symbol, String] a field name
        # @return [Boolean] whether the field is present
        def key?(key)
          @values.key?(key.to_s.to_sym)
        end

        # @return [Array<Symbol>] all field names
        def keys
          @values.keys
        end

        # Iterates over field name/value pairs.
        #
        # @yield [name, value]
        # @return [Enumerator, void]
        def each(&block)
          @values.each(&block)
        end

        # @return [Hash{Symbol => Object}] the values as a plain hash
        def to_h
          @values.dup
        end

        # @param other [Object] another object
        # @return [Boolean] whether the values and input keys match
        def ==(other)
          other.is_a?(Example) && other.values == @values && other.input_keys == @input_keys
        end
        alias eql? ==

        # @return [Integer] hash code consistent with {#==}
        def hash
          [@values, @input_keys].hash
        end

        # @return [String] string representation
        def to_s
          "#<Example #{@values.inspect}>"
        end
        alias inspect to_s

        # Reads a field by name.
        #
        # @param name [Symbol] the field name
        # @return [Object] the value
        # @raise [NoMethodError] if the field is absent
        def method_missing(name, *args)
          return @values[name] if args.empty? && @values.key?(name)

          super
        end

        # @param name [Symbol] the method name
        # @param include_private [Boolean] whether to consider private methods
        # @return [Boolean] whether the name matches a field
        def respond_to_missing?(name, include_private = false)
          @values.key?(name) || super
        end

        protected

        # @param keys [Array<Symbol>] the new input keys
        # @return [Array<Symbol>] the input keys
        attr_writer :input_keys

        private

        # @param values [Hash] the values for the copy
        # @return [Example] a new example of the same class
        def dup_with(values)
          self.class.new(**values)
        end

        # @param values [Hash] a hash with string or symbol keys
        # @return [Hash{Symbol => Object}] the same hash with symbol keys
        def normalize(values)
          values.each_with_object({}) { |(key, value), result| result[key.to_s.to_sym] = value }
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
