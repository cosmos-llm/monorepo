# frozen_string_literal: true

require_relative 'adapters/chat_adapter'
require_relative 'cache'

module Cosmos
  module Llm
    module Predict
      # Process-wide defaults for client, adapter, and cache.
      #
      # Modules read from here when not given explicit collaborators, so an
      # application configures the LM once and every predictor picks it up. This
      # is a deliberate convenience, and it is the reason every module still
      # accepts explicit overrides: tests and optimizers need to swap the client
      # without touching global state.
      #
      # @example Configure once at boot
      #   Cosmos::Llm::Predict.configure do |settings|
      #     settings.client = Cosmos::Llm::Client.new(:anthropic, model: 'claude-opus-4')
      #     settings.cache = Cosmos::Llm::Predict::Cache.new(directory: '~/.cache/cosmos-llm')
      #   end
      #
      # @example Override temporarily
      #   Cosmos::Llm::Predict.with(temperature: 1.0) { predictor.call(question: q) }
      class Settings
        # @return [Object, nil] the default client
        attr_accessor :client

        # @return [Adapters::Base] the default adapter
        attr_accessor :adapter

        # @return [Cache, nil] the default cache
        attr_accessor :cache

        # @return [String, nil] the default model name
        attr_accessor :model

        # @return [Float, nil] the default sampling temperature
        attr_accessor :temperature

        # @return [Integer] the default max_tokens
        attr_accessor :max_tokens

        # @return [Integer] how many times to retry a failed parse
        attr_accessor :max_parse_retries

        # Builds settings with library defaults.
        def initialize
          @client = nil
          @adapter = Adapters::ChatAdapter.new
          @cache = nil
          @model = nil
          @temperature = nil
          @max_tokens = 4096
          @max_parse_retries = 1
        end

        # @return [Hash] the overridable values as a hash
        def to_h
          {
            client: @client,
            adapter: @adapter,
            cache: @cache,
            model: @model,
            temperature: @temperature,
            max_tokens: @max_tokens,
            max_parse_retries: @max_parse_retries
          }
        end

        # Applies a hash of overrides in place.
        #
        # @param overrides [Hash] values to set
        # @return [void]
        def apply(overrides)
          overrides.each { |key, value| public_send(:"#{key}=", value) }
        end
      end

      class << self
        # The current settings.
        #
        # @return [Settings] the active settings object
        def settings
          @settings ||= Settings.new
        end

        # Configures the process-wide defaults.
        #
        # @yield [Settings] the settings object
        # @return [Settings] the settings object
        # @example
        #   Cosmos::Llm::Predict.configure { |s| s.max_tokens = 8192 }
        def configure
          yield settings if block_given?
          settings
        end

        # Runs a block with temporarily overridden settings.
        #
        # Overrides are restored afterward even if the block raises, so a failed
        # call cannot leave the process configured differently than it found it.
        #
        # @param overrides [Hash] settings to override for the duration
        # @yield the block to run
        # @return [Object] the block's return value
        # @example
        #   Cosmos::Llm::Predict.with(temperature: 1.0) { predictor.call(q) }
        def with(**overrides)
          previous = settings.to_h
          settings.apply(overrides)
          yield
        ensure
          settings.apply(previous)
        end

        # Resets settings to library defaults.
        #
        # @return [Settings] fresh settings
        def reset_settings!
          @settings = Settings.new
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
