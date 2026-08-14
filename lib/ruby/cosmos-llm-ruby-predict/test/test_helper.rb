# frozen_string_literal: true

$LOAD_PATH.unshift File.expand_path('../lib', __dir__)
$LOAD_PATH.unshift File.expand_path('../../cosmos-llm-ruby-signature/lib', __dir__)

require 'cosmos/llm/predict'

require 'minitest/autorun'

# A stand-in for Cosmos::Llm::Client that returns scripted completions.
#
# Tests drive the whole stack — adapter, parsing, retries, caching — without a
# network call. Requests are recorded so a test can assert on what was sent.
class FakeClient
  # A response matching the provider-neutral shape: +text+ and +usage+.
  FakeResponse = Struct.new(:text, :usage)

  # @return [Array<Hash>] every request this client received
  attr_reader :requests

  # @param replies [Array<String>] completions to return, in order; the last one
  #   repeats once exhausted
  def initialize(*replies)
    @replies = replies.flatten
    @requests = []
  end

  # @param params [Hash] the completion parameters
  # @return [FakeResponse] the next scripted reply
  def completion(**params)
    @requests << params
    reply = @replies.length > 1 ? @replies.shift : @replies.first
    raise reply if reply.is_a?(StandardError)

    FakeResponse.new(reply.to_s, { 'input_tokens' => 10, 'output_tokens' => 5 })
  end

  # @return [Integer] how many completions were requested
  def call_count
    @requests.length
  end
end

module Minitest
  class Test
    def teardown
      Cosmos::Llm::Predict.reset_settings!
      super
    end
  end
end
