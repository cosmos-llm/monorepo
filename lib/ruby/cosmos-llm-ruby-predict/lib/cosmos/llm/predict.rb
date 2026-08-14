# frozen_string_literal: true

require 'cosmos/llm/signature'

require_relative 'predict/version'
require_relative 'predict/errors'
require_relative 'predict/example'
require_relative 'predict/prediction'
require_relative 'predict/cache'
require_relative 'predict/adapters/base'
require_relative 'predict/adapters/chat_adapter'
require_relative 'predict/adapters/json_adapter'
require_relative 'predict/json_stream'
require_relative 'predict/settings'
require_relative 'predict/module'
require_relative 'predict/predictor'
require_relative 'predict/chain_of_thought'
require_relative 'predict/best_of_n'

module Cosmos
  module Llm
    # Runs signatures against language models.
    #
    # Where {Cosmos::Llm::Signature} says what a call does, this gem does it: an
    # adapter renders the signature into messages, a client sends them, and the
    # adapter parses the reply back into typed values. The result is a
    # {Predict::Prediction}.
    #
    # @example Configure once, then call
    #   Cosmos::Llm::Predict.configure do |settings|
    #     settings.client = Cosmos::Llm::Client.new(:anthropic, model: 'claude-opus-4')
    #   end
    #
    #   answer = Cosmos::Llm::Predict::Predict.new('question -> answer')
    #   answer.call(question: 'What is 2+2?').answer  # => '4'
    #
    # @see Predict::Predict
    # @see Predict::ChainOfThought
    # @see Predict::BestOfN
    module Predict
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
