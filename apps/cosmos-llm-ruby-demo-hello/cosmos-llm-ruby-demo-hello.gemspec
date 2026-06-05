# frozen_string_literal: true

require_relative "lib/cosmos_llm_hello/version"

Gem::Specification.new do |spec|
  spec.name        = "cosmos-llm-ruby-demo-hello"
  spec.version     = CosmosLlmHello::VERSION
  spec.authors     = ["Durable Programming LLC"]
  spec.email       = ["commercial@cosmos-llm.com"]
  spec.summary     = "A simple CLI chatbot built on cosmos-llm"
  spec.description = "An interactive command-line chatbot that demonstrates cosmos-llm's multi-provider LLM interface."
  spec.homepage    = "https://github.com/cosmos-llm/cosmos-llm-ruby-demo-hello"
  spec.license     = "MIT"

  spec.required_ruby_version = ">= 3.0.0"

  spec.metadata = {
    "homepage_uri" => spec.homepage,
    "source_code_uri" => spec.homepage,
    "changelog_uri" => "#{spec.homepage}/blob/main/CHANGELOG.md",
    "rubygems_mfa_required" => "true"
  }

  spec.files       = Dir["lib/**/*", "exe/**/*", "LICENSE.txt", "README.md", "CHANGELOG.md"]
  spec.bindir      = "exe"
  spec.executables = ["durable-llm-hello"]

  spec.add_dependency "cosmos-llm", ">= 0.1.4"

  spec.add_development_dependency "minitest", "~> 5"
  spec.add_development_dependency "rake", "~> 13"
  spec.add_development_dependency "rubocop", "~> 1.82"
  spec.add_development_dependency "simplecov"
  spec.add_development_dependency "yard", "~> 0.9"
end
