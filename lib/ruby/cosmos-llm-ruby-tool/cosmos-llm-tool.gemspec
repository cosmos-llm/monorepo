# frozen_string_literal: true

require_relative 'lib/cosmos/llm/tool/version'

Gem::Specification.new do |spec|
  spec.name = 'cosmos-llm-tool'
  spec.version = Cosmos::Llm::Tool::VERSION
  spec.authors = ['Durable Programming Team']
  spec.email = ['djberube@cosmos-llm.com']

  spec.summary = 'A comprehensive tool system for cosmos-llm'
  spec.description = 'Durable-LLM-Tool provides a flexible and extensible tool system for Large Language Models, enabling function calling, tool registration, and execution management for agentic LLM interactions.'
  spec.homepage = 'https://github.com/cosmos-llm/cosmos-llm-tool'
  spec.license = 'MIT'
  spec.required_ruby_version = '>= 2.6.0'

  spec.metadata['allowed_push_host'] = 'https://rubygems.org'

  spec.metadata['homepage_uri'] = spec.homepage
  spec.metadata['source_code_uri'] = 'https://github.com/cosmos-llm/cosmos-llm-tool'
  spec.metadata['changelog_uri'] = 'https://github.com/cosmos-llm/cosmos-llm-tool/blob/main/CHANGELOG.md'

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

  spec.add_dependency 'cosmos-llm', '~> 0.1'
  spec.add_dependency 'zeitwerk', '~> 2.6', '>= 2.6.0'

  spec.add_development_dependency 'dotenv', '~> 2.8'
  spec.add_development_dependency 'minitest', '~> 5.0'
  spec.add_development_dependency 'mocha', '~> 2.1'
  spec.add_development_dependency 'rubocop', '~> 1.0'
  spec.add_development_dependency 'yard', '~> 0.9'
end
