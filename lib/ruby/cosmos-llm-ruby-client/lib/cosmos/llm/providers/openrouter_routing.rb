# frozen_string_literal: true

# Provider-routing preferences for the OpenRouter API.
#
# OpenRouter serves one model identifier from any of several upstream providers,
# and they are not interchangeable: they differ in price, throughput, context
# length, quantization, whether they retain prompts for training, and whether
# they honour a parameter the request depends on. Left alone, OpenRouter load
# balances across the ones it considers healthy. This class is how a caller
# overrides that.

require 'cosmos/llm/errors'

module Cosmos
  module Llm
    module Providers
      class OpenRouter < Cosmos::Llm::Providers::Base
        # Builds the `provider` block of an OpenRouter chat completion request.
        #
        # Every setting is optional, and an untouched Routing serializes to an
        # empty hash, which leaves OpenRouter's default behaviour alone.
        #
        # Provider names are OpenRouter's slugs -- "anthropic", "together",
        # "azure" -- not the vendor prefix of a model id. The two look alike and
        # are different namespaces: "anthropic/claude-3.5-sonnet" is a model,
        # "anthropic" is the upstream serving it.
        #
        # OpenRouter rejects unknown keys in this block with a 400 rather than
        # ignoring them, so the setters validate names and values up front. A
        # typo fails here, with a message naming the valid options, instead of
        # failing as an opaque API error one network round trip later.
        #
        # @example Pin a model to one upstream, failing rather than falling back
        #   routing = Cosmos::Llm::Providers::OpenRouter::Routing.pinned_to('together')
        #   client.completion(messages: msgs, **routing.to_params)
        #
        # @example Rank a preference order and exclude one provider
        #   routing = Cosmos::Llm::Providers::OpenRouter::Routing.new
        #                                                       .order('azure', 'openai')
        #                                                       .ignore('deepinfra')
        #
        # @example Take the cheapest provider that can serve the request
        #   routing = Cosmos::Llm::Providers::OpenRouter::Routing.new.sort(:price)
        class Routing
          # Attributes OpenRouter can rank candidate providers by.
          #
          # Setting a sort replaces OpenRouter's default load balancing, which
          # weighs price against recent reliability, with a single-attribute
          # ordering.
          SORTS = %w[price throughput latency exacto].freeze

          # How a sort is applied across a fallback model list. Only meaningful
          # alongside a `models` fallback list.
          SORT_PARTITIONS = %w[model none].freeze

          # Whether providers that retain and train on prompts are eligible.
          DATA_COLLECTION = %w[allow deny].freeze

          # Quantization levels a model may be served at. Lower precision is
          # cheaper and measurably worse; this is how a caller who cares refuses
          # the cheap one.
          QUANTIZATIONS = %w[
            int4 int8 fp4 mxfp4 nvfp4 fp6 fp8 mxfp8 fp16 bf16 fp32 unknown
          ].freeze

          # Price ceilings accepted by `max_price`. `prompt` and `completion` are
          # dollars per million tokens; `request`, `image`, and `audio` are
          # dollars per unit.
          PRICE_KEYS = %i[prompt completion request image audio].freeze

          # Builds a routing preference pinned to a single provider, with
          # fallbacks disabled.
          #
          # The strict reading of "pin": the named provider serves the request or
          # it fails. Use {#only} alone to allow the rest of the field as backup.
          #
          # @param provider [String, Symbol] an OpenRouter provider slug
          # @return [Routing]
          def self.pinned_to(provider)
            new.only(provider).allow_fallbacks(false)
          end

          # Builds a Routing from a plain hash, as {#to_h} produces.
          #
          # Accepts both spellings of `sort`: the plain string, and the
          # {'by' =>, 'partition' =>} object a partition promotes it to.
          #
          # @param hash [Hash] a `provider` block
          # @return [Routing]
          # @raise [ArgumentError] if a key or value is not valid
          def self.from_h(hash)
            hash = (hash || {}).transform_keys(&:to_sym)
            routing = new

            SPLAT_SETTERS.each do |key|
              routing.public_send(key, *Array(hash[key])) if hash.key?(key)
            end

            SCALAR_SETTERS.each do |key|
              routing.public_send(key, hash[key]) if hash.key?(key)
            end

            routing.max_price(**hash[:max_price].transform_keys(&:to_sym)) if hash[:max_price]
            apply_sort(routing, hash[:sort]) if hash[:sort]

            routing
          end

          # Setters taking a variable-length list of names.
          SPLAT_SETTERS = %i[order only ignore quantizations].freeze

          # Setters taking a single value.
          SCALAR_SETTERS = %i[
            allow_fallbacks require_parameters data_collection zdr
            enforce_distillable_text preferred_min_throughput preferred_max_latency
          ].freeze

          # Replays a `sort` in either spelling: the plain string, or the
          # {'by' =>, 'partition' =>} object that a partition promotes it to.
          def self.apply_sort(routing, sort)
            if sort.is_a?(Hash)
              sort = sort.transform_keys(&:to_sym)
              routing.sort(sort[:by], partition: sort[:partition])
            else
              routing.sort(sort)
            end
          end
          private_class_method :apply_sort

          def initialize
            @block = {}
          end

          # Sets the preference order, most preferred first.
          #
          # Providers not named here remain eligible as fallbacks unless
          # {#allow_fallbacks} is set to false. Replaces any order already set.
          #
          # @param providers [Array<String, Symbol>] provider slugs
          # @return [self]
          def order(*providers)
            set_list(:order, providers)
          end

          # Restricts routing to the named providers.
          #
          # Anything not named is excluded outright, which is the difference
          # between this and {#order}. Replaces any allow-list already set.
          #
          # @param providers [Array<String, Symbol>] provider slugs
          # @return [self]
          def only(*providers)
            set_list(:only, providers)
          end

          # Excludes the named providers. Replaces any deny-list already set.
          #
          # @param providers [Array<String, Symbol>] provider slugs
          # @return [self]
          def ignore(*providers)
            set_list(:ignore, providers)
          end

          # Sets whether OpenRouter may fall back to a provider outside {#order}
          # or {#only}.
          #
          # False makes the request fail rather than route somewhere unlisted.
          # OpenRouter allows fallbacks by default.
          #
          # @param allow [Boolean]
          # @return [self]
          def allow_fallbacks(allow)
            set_bool(:allow_fallbacks, allow)
          end

          # Restricts routing to providers supporting every parameter the request
          # sets.
          #
          # Without this, a provider that ignores (say) `stop` can still serve the
          # call, and the parameter is silently dropped.
          #
          # @param require [Boolean]
          # @return [self]
          def require_parameters(require)
            set_bool(:require_parameters, require)
          end

          # Sets whether prompt-retaining providers are eligible.
          #
          # @param policy [String, Symbol] "allow" or "deny"
          # @return [self]
          # @raise [ArgumentError] if the policy is not recognized
          def data_collection(policy)
            @block[:data_collection] = validate_enum(policy, DATA_COLLECTION, 'data_collection')
            self
          end

          # Ranks candidate providers by a single attribute.
          #
          # Setting this replaces OpenRouter's default load balancing. When
          # `order` is also set, `order` takes precedence.
          #
          # @param attribute [String, Symbol] one of {SORTS}
          # @param partition [String, Symbol, nil] one of {SORT_PARTITIONS};
          #   only meaningful alongside a `models` fallback list
          # @return [self]
          # @raise [ArgumentError] if the attribute or partition is not recognized
          def sort(attribute, partition: nil)
            by = validate_enum(attribute, SORTS, 'sort')

            @block[:sort] = if partition.nil?
                              by
                            else
                              {
                                'by' => by,
                                'partition' => validate_enum(partition, SORT_PARTITIONS,
                                                             'sort partition')
                              }
                            end
            self
          end

          # Restricts routing to the named quantization levels.
          #
          # @param levels [Array<String, Symbol>] one or more of {QUANTIZATIONS}
          # @return [self]
          # @raise [ArgumentError] if a level is not recognized
          def quantizations(*levels)
            levels = levels.flatten.map { |l| validate_enum(l, QUANTIZATIONS, 'quantization') }
            @block[:quantizations] = levels unless levels.empty?
            self
          end

          # Sets per-token price ceilings. Providers above any ceiling are
          # dropped, and the request fails if none qualifies.
          #
          # `prompt` and `completion` are dollars per million tokens; `request`,
          # `image`, and `audio` are dollars per unit.
          #
          # @param ceilings [Hash] any of {PRICE_KEYS} to a number
          # @return [self]
          # @raise [ArgumentError] if a key is not recognized or a value is not
          #   a non-negative number
          def max_price(**ceilings)
            ceilings.each do |key, value|
              unless PRICE_KEYS.include?(key)
                raise ArgumentError,
                      "Unknown max_price key #{key.inspect}.\n" \
                      "Valid keys: #{PRICE_KEYS.join(', ')}."
              end

              validate_number(value, "max_price #{key}")
            end

            @block[:max_price] = ceilings.transform_keys(&:to_s) unless ceilings.empty?
            self
          end

          # Restricts routing to zero-data-retention providers.
          #
          # Stricter than `data_collection: "deny"`, which only rules out training
          # on the prompt.
          #
          # @param require [Boolean]
          # @return [self]
          def zdr(require)
            set_bool(:zdr, require)
          end

          # Restricts routing to providers whose terms permit using the output to
          # train another model.
          #
          # @param require [Boolean]
          # @return [self]
          def enforce_distillable_text(require)
            set_bool(:enforce_distillable_text, require)
          end

          # Deprioritizes providers below this throughput, in tokens per second.
          #
          # A preference, not a filter: unlike {#max_price}, a provider under the
          # bar is ranked down rather than excluded, so the request will not fail
          # for want of a fast enough provider.
          #
          # @param tokens_per_second [Numeric]
          # @return [self]
          def preferred_min_throughput(tokens_per_second)
            @block[:preferred_min_throughput] =
              validate_number(tokens_per_second, 'preferred_min_throughput')
            self
          end

          # Deprioritizes providers slower than this time to first token, in
          # seconds. A preference, not a filter -- see
          # {#preferred_min_throughput}.
          #
          # @param seconds [Numeric]
          # @return [self]
          def preferred_max_latency(seconds)
            @block[:preferred_max_latency] = validate_number(seconds, 'preferred_max_latency')
            self
          end

          # @return [Boolean] true when no preference of any kind is set
          def empty?
            @block.empty?
          end

          # @return [Hash] the `provider` block, with string keys
          def to_h
            @block.transform_keys(&:to_s)
          end

          # Returns the routing wrapped for splatting into a request.
          #
          # An empty routing produces an empty hash rather than `provider: {}`,
          # so an untouched Routing changes nothing about the request.
          #
          # @return [Hash] `{ provider: {...} }`, or `{}` when empty
          # @example
          #   client.completion(messages: msgs, **routing.to_params)
          def to_params
            empty? ? {} : { provider: to_h }
          end

          # @return [String]
          def to_s
            "#<#{self.class.name} #{to_h.inspect}>"
          end
          alias inspect to_s

          # @return [Boolean]
          def ==(other)
            other.is_a?(Routing) && other.to_h == to_h
          end
          alias eql? ==

          # @return [Integer]
          def hash
            to_h.hash
          end

          private

          def set_list(key, providers)
            names = providers.flatten.map { |p| validate_slug(p, key) }
            @block[key] = names unless names.empty?
            self
          end

          def set_bool(key, value)
            unless [true, false].include?(value)
              raise ArgumentError, "#{key} must be true or false, got #{value.inspect}."
            end

            @block[key] = value
            self
          end

          def validate_slug(name, key)
            slug = name.to_s.strip
            raise ArgumentError, "#{key} entries must be non-empty provider slugs." if slug.empty?

            slug
          end

          def validate_enum(value, allowed, label)
            normalized = value.to_s.downcase
            return normalized if allowed.include?(normalized)

            raise ArgumentError,
                  "Unknown #{label} #{value.inspect}.\n" \
                  "Valid values: #{allowed.join(', ')}."
          end

          def validate_number(value, label)
            unless value.is_a?(Numeric) && value >= 0
              raise ArgumentError, "#{label} must be a non-negative number, got #{value.inspect}."
            end

            value
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
