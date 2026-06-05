# frozen_string_literal: true

require_relative "lib/cosmos_llm_markdowner/version"

Gem::Specification.new do |spec|
  spec.name        = "cosmos-llm-ruby-demo-markdowner"
  spec.version     = CosmosLlmMarkdowner::VERSION
  spec.authors     = ["Durable Programming LLC"]
  spec.email       = ["commercial@cosmos-llm.com"]
  spec.summary     = "Agentic CLI that uses an LLM to write markdown files"
  spec.description = "An interactive command-line tool that uses an LLM agent to write markdown files, " \
                     "with explicit filesystem permissions: reads and writes are denied unless the user " \
                     "allows them via --in, --out, or --out-dir flags."
  spec.homepage    = "https://github.com/cosmos-llm/cosmos-llm-ruby-demo-markdowner"
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
  spec.executables = ["durable-llm-markdowner"]

  spec.add_dependency "cosmos-llm",      ">= 0.1.4"
  spec.add_dependency "cosmos-llm-tool", ">= 0.1.0"

  spec.add_development_dependency "minitest",  "~> 5"
  spec.add_development_dependency "rake",      "~> 13"
  spec.add_development_dependency "rubocop",   "~> 1.82"
  spec.add_development_dependency "simplecov"
  spec.add_development_dependency "yard", "~> 0.9"
end
