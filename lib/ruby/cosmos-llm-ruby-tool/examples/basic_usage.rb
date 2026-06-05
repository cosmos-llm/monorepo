#!/usr/bin/env ruby
# frozen_string_literal: true

require_relative '../lib/cosmos/llm/tool'

# Example 1: Simple calculator tool
calculator = Cosmos::Llm::Tool.define(:calculator) do
  description 'Performs basic arithmetic operations'

  parameter :operation, type: :string, enum: %w[add subtract multiply divide], required: true
  parameter :a, type: :number, required: true
  parameter :b, type: :number, required: true

  execute do |params|
    a = params[:a]
    b = params[:b]
    case params[:operation]
    when 'add' then a + b
    when 'subtract' then a - b
    when 'multiply' then a * b
    when 'divide' then b.zero? ? 'Error: Division by zero' : a / b
    end
  end
end

puts "=== Calculator Tool ==="
puts "5 + 3 = #{calculator.call(operation: 'add', a: 5, b: 3)}"
puts "10 - 4 = #{calculator.call(operation: 'subtract', a: 10, b: 4)}"
puts "6 * 7 = #{calculator.call(operation: 'multiply', a: 6, b: 7)}"
puts "20 / 4 = #{calculator.call(operation: 'divide', a: 20, b: 4)}"
puts "\n"

# Example 2: Weather tool (mock)
weather = Cosmos::Llm::Tool.define(:weather) do
  description 'Get current weather for a location'

  parameter :location, type: :string, required: true
  parameter :units, type: :string, enum: %w[celsius fahrenheit], default: 'celsius'

  execute do |params|
    # Mock weather data
    temp = params[:units] == 'celsius' ? 22 : 72
    {
      location: params[:location],
      temperature: temp,
      units: params[:units],
      condition: 'Sunny'
    }
  end
end

puts "=== Weather Tool ==="
result = weather.call(location: 'San Francisco')
puts "Weather in #{result[:location]}: #{result[:temperature]}°#{result[:units][0].upcase} - #{result[:condition]}"
puts "\n"

# Example 3: String manipulation tool
string_tool = Cosmos::Llm::Tool.define(:string_manipulator) do
  description 'Manipulates strings in various ways'

  parameter :text, type: :string, required: true
  parameter :operation, type: :string, enum: %w[uppercase lowercase reverse length], required: true

  execute do |params|
    text = params[:text]
    case params[:operation]
    when 'uppercase' then text.upcase
    when 'lowercase' then text.downcase
    when 'reverse' then text.reverse
    when 'length' then text.length
    end
  end
end

puts "=== String Manipulator Tool ==="
puts "Uppercase: #{string_tool.call(text: 'hello world', operation: 'uppercase')}"
puts "Reverse: #{string_tool.call(text: 'hello world', operation: 'reverse')}"
puts "Length: #{string_tool.call(text: 'hello world', operation: 'length')}"
puts "\n"

# Example 4: Schema Generation
puts "=== OpenAI Schema for Calculator ==="
require 'json'
puts JSON.pretty_generate(calculator.to_openai_schema)
puts "\n"

puts "=== Anthropic Schema for Weather ==="
puts JSON.pretty_generate(weather.to_anthropic_schema)
puts "\n"

puts "=== JSON Schema for String Manipulator ==="
puts JSON.pretty_generate(string_tool.to_json_schema)
puts "\n"

# Example 5: Registry operations
puts "=== Tool Registry ==="
puts "Total tools registered: #{Cosmos::Llm::Tool.all.length}"
puts "Tool names: #{Cosmos::Llm::Tool.all.map(&:name).join(', ')}"
puts "\n"

# Access tools from registry
calc_from_registry = Cosmos::Llm::Tool.get(:calculator)
puts "Retrieved calculator from registry: #{calc_from_registry.name}"
puts "Result: 100 + 50 = #{calc_from_registry.call(operation: 'add', a: 100, b: 50)}"

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
