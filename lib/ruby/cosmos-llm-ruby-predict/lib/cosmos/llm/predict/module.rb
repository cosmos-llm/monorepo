# frozen_string_literal: true

require_relative 'prediction'

module Cosmos
  module Llm
    module Predict
      # Base class for anything callable that produces a {Prediction}.
      #
      # A module is the composable unit: {Predict} calls a model once, and larger
      # modules built from it (retry wrappers, best-of-N samplers, whole
      # pipelines) are modules too. What they share is the calling convention —
      # keyword inputs in, a {Prediction} out — and the ability to enumerate the
      # {Predict} instances nested inside them.
      #
      # That enumeration is what makes optimization possible later: an optimizer
      # needs to find every predictor in a program so it can install
      # demonstrations or swap instructions without knowing the program's shape.
      #
      # @abstract Subclass and implement {#forward}.
      # @example A two-step pipeline
      #   class Rag < Cosmos::Llm::Predict::Module
      #     def initialize
      #       @generate = Cosmos::Llm::Predict::Predict.new('context, question -> answer')
      #     end
      #
      #     def forward(question:)
      #       @generate.call(context: retrieve(question), question: question)
      #     end
      #   end
      class Module
        # Runs the module.
        #
        # @param inputs [Hash] the input field values
        # @return [Prediction] the result
        # @raise [NotImplementedError] unless {#forward} is implemented
        def call(**inputs)
          forward(**inputs)
        end

        # The module's actual work.
        #
        # @abstract
        # @param inputs [Hash] the input field values
        # @return [Prediction] the result
        # @raise [NotImplementedError] unless overridden
        def forward(**inputs)
          raise NotImplementedError, "#{self.class} must implement #forward"
        end

        # Every {Predict} reachable from this module, with dotted paths.
        #
        # Walks instance variables, including through arrays and hashes, so a
        # module holding a list of sub-modules is enumerated correctly. Cycles
        # are tracked by object identity and visited once.
        #
        # @return [Hash{String => Predict}] predictors keyed by path
        # @example
        #   pipeline.named_predictors.keys # => ["generate", "critique"]
        def named_predictors
          found = {}
          visit(self, '', found, {}.compare_by_identity)
          found
        end

        # @return [Array<Predict>] every {Predict} reachable from this module
        def predictors
          named_predictors.values
        end

        # A deep copy of this module, sharing no mutable predictor state.
        #
        # Optimizers rely on this: they need to try a variant without disturbing
        # the program they were handed.
        #
        # @return [Module] an independent copy
        def deep_copy
          Marshal.load(Marshal.dump(self))
        rescue TypeError
          copy_by_hand
        end

        # The tunable state of every predictor, for saving or inspection.
        #
        # @return [Hash{String => Hash}] predictor state keyed by path
        def dump_state
          named_predictors.transform_values(&:dump_state)
        end

        # Restores predictor state produced by {#dump_state}.
        #
        # @param state [Hash{String => Hash}] the state to restore
        # @return [self]
        # @raise [ConfigurationError] if the state does not match this module
        def load_state(state)
          predictors = named_predictors
          state.each do |path, predictor_state|
            predictor = predictors[path]
            raise ConfigurationError, "No predictor at path #{path.inspect} in #{self.class}" unless predictor

            predictor.load_state(predictor_state)
          end
          self
        end

        private

        # Recursively collects predictors from an object's instance variables.
        #
        # @param object [Object] the object to walk
        # @param prefix [String] the path prefix
        # @param found [Hash] accumulator for discovered predictors
        # @param seen [Hash] identity set of already-visited objects
        # @return [void]
        def visit(object, prefix, found, seen)
          return if seen.key?(object)

          seen[object] = true

          object.instance_variables.each do |ivar|
            name = ivar.to_s.delete_prefix('@')
            value = object.instance_variable_get(ivar)
            collect(value, join_path(prefix, name), found, seen)
          end
        end

        # Records a value if it is a predictor, and descends if it is a container
        # or another module.
        #
        # @param value [Object] the value to inspect
        # @param path [String] the path to this value
        # @param found [Hash] accumulator for discovered predictors
        # @param seen [Hash] identity set of already-visited objects
        # @return [void]
        def collect(value, path, found, seen)
          case value
          when Predict
            found[path] = value
          when Module
            visit(value, path, found, seen)
          when Array
            value.each_with_index { |element, index| collect(element, "#{path}[#{index}]", found, seen) }
          when Hash
            value.each { |key, element| collect(element, "#{path}[#{key}]", found, seen) }
          end
        end

        # @param prefix [String] the existing path
        # @param name [String] the segment to append
        # @return [String] the joined path
        def join_path(prefix, name)
          prefix.empty? ? name : "#{prefix}.#{name}"
        end

        # Fallback copy for modules holding unmarshalable state such as an open
        # HTTP client. Copies instance variables shallowly, then deep-copies the
        # predictors, which are the only parts an optimizer mutates.
        #
        # @return [Module] an independent copy
        def copy_by_hand
          copy = clone
          instance_variables.each do |ivar|
            value = instance_variable_get(ivar)
            copy.instance_variable_set(ivar, value.is_a?(Module) || value.is_a?(Predict) ? value.deep_copy : value)
          end
          copy
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
