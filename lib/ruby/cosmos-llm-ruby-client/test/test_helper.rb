# frozen_string_literal: true

ENV.delete_if { |key, _| key.start_with?('CLLM') }
require 'bundler/setup'

$LOAD_PATH.unshift File.expand_path('../lib', __dir__)
require 'cosmos/llm'

Cosmos::Llm.configuration.clear

require 'minitest/autorun'
