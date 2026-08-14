# frozen_string_literal: true

require_relative 'lib/cosmos/llm/signature/version'

Gem::Specification.new do |spec|
  spec.name = 'cosmos-llm-signature'
  spec.version = Cosmos::Llm::Signature::VERSION
  spec.authors = ['Durable Programming Team']
  spec.email = ['djberube@cosmos-llm.com']

  spec.summary = 'Declarative typed input/output signatures for LLM calls'
  spec.description = 'Cosmos-LLM-Signature provides a declarative way to describe a single LLM call as named typed inputs, named typed outputs, and instructions, with JSON schema derivation and response coercion.'
  spec.homepage = 'https://github.com/cosmos-llm/cosmos-llm-signature'
  spec.license = 'MIT'
  spec.required_ruby_version = '>= 2.6.0'

  spec.metadata['allowed_push_host'] = 'https://rubygems.org'

  spec.metadata['homepage_uri'] = spec.homepage
  spec.metadata['source_code_uri'] = 'https://github.com/cosmos-llm/cosmos-llm-signature'
  spec.metadata['changelog_uri'] = 'https://github.com/cosmos-llm/cosmos-llm-signature/blob/main/CHANGELOG.md'

  spec.files = Dir.chdir(__dir__) do
    if system('git rev-parse --git-dir > /dev/null 2>&1')
      `git ls-files -z`.split("\x0").reject do |f|
        (File.expand_path(f) == __FILE__) || f.start_with?(*%w[bin/ test/ spec/ features/ .git .circleci appveyor])
      end
    else
      Dir.glob('**/*', File::FNM_DOTMATCH).reject do |f|
        File.directory?(f) || (File.expand_path(f) == __FILE__) || f.start_with?(*%w[bin/ test/ spec/ features/ .git .circleci appveyor])
      end
    end
  end
  spec.bindir = 'exe'
  spec.executables = spec.files.grep(%r{\Aexe/}) { |f| File.basename(f) }
  spec.require_paths = ['lib']

  spec.add_development_dependency 'minitest', '~> 5.0'
  spec.add_development_dependency 'rubocop', '~> 1.0'
  spec.add_development_dependency 'yard', '~> 0.9'
end
