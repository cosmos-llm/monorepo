# frozen_string_literal: true

require 'durable/llm'
require 'durable/llm/client'

client = Cosmos::Llm::Client.new(:openai, model: 'gpt-4')

response = client.complete("What's the capital of California?")

puts response
