# frozen_string_literal: true

# Main entry point for the Cosmos-LLM gem.
#
# This module provides a namespace for Cosmos-LLM, part of the Cosmos LLM series.
# It uses autoloading for efficient memory usage and lazy loading of components.
#
# Currently, it provides access to the LLM functionality through the Llm submodule.
#
# @example Basic usage
#   require 'cosmos-llm'
#
#   # Access LLM functionality
#   Cosmos::Llm.configure do |config|
#     config.openai.api_key = 'your-key'
#   end
#
#   client = Cosmos::Llm.new(:openai)
#   response = client.complete('Hello!')
#
# @see Cosmos::Llm

# Namespace module for Cosmos-LLM.
#
# This module serves as the root namespace for Cosmos-LLM, providing
# autoloaded access to various components and functionality.
# Part of the Cosmos LLM series. Enterprise support available from Durable Programming.
module Cosmos
  # Autoload the Llm module for lazy loading
  autoload :Llm, 'cosmos/llm'
end

# Copyright (c) 2025 Cosmos LLM. All rights reserved.
