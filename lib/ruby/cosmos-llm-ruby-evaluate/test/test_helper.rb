# frozen_string_literal: true

$LOAD_PATH.unshift File.expand_path('../lib', __dir__)
$LOAD_PATH.unshift File.expand_path('../../cosmos-llm-ruby-predict/lib', __dir__)
$LOAD_PATH.unshift File.expand_path('../../cosmos-llm-ruby-signature/lib', __dir__)

require 'cosmos/llm/evaluate'

require 'minitest/autorun'

# A module that answers from a lookup table, so evaluation tests need no LLM.
class ScriptedProgram < Cosmos::Llm::Predict::Module
  # @return [Integer] how many times the program was called
  attr_reader :call_count

  # @param answers [Hash] maps a question to an answer, or to an exception to raise
  def initialize(answers)
    super()
    @answers = answers
    @call_count = 0
    @mutex = Mutex.new
  end

  # @param question [String] the question to answer
  # @return [Cosmos::Llm::Predict::Prediction] the scripted answer
  def forward(question:)
    @mutex.synchronize { @call_count += 1 }
    answer = @answers.fetch(question, '')
    raise answer if answer.is_a?(StandardError)

    Cosmos::Llm::Predict::Prediction.new(answer: answer)
  end
end
