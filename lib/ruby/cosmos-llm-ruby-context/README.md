# Cosmos LLM Context

[![Gem Version](https://badge.fury.io/rb/cosmos-llm-context.svg)](https://badge.fury.io/rb/cosmos-llm-context)

A Ruby DSL for modeling and managing LLM agentic contexts with virtual filesystems, structured blocks, and multiple output renderers.

## Overview

Cosmos LLM Context provides a comprehensive, modular approach to building contexts for Large Language Models. It enables you to:

- **Build Virtual Filesystems**: Create hierarchical file structures to represent project layouts
- **Compose Context Blocks**: Organize system prompts, user messages, and custom content
- **Multi-Format Rendering**: Export contexts for Anthropic Claude, OpenAI, XML, JSON, and custom formats
- **Immutable Design**: Thread-safe, predictable context objects
- **Extensible Architecture**: Register custom renderers and extend functionality via mixins

## Installation

Add this line to your application's Gemfile:

```ruby
gem 'cosmos-llm-context'
```

And then execute:

```bash
$ bundle install
```

Or install it yourself as:

```bash
$ gem install cosmos-llm-context
```

## Requirements

- Ruby >= 2.6.0
- Zeitwerk ~> 2.6

## Quick Start

```ruby
require 'cosmos/llm/context'

# Build a context with blocks and virtual filesystem
context = Cosmos::Llm::Context.build do
  # Add a system prompt
  block :system, 'You are a helpful Ruby programming assistant.'

  # Add a user message
  block :user, 'Help me create a simple Ruby project structure.'

  # Define a virtual filesystem
  filesystem do
    file 'Gemfile', content: <<~GEMFILE
      source 'https://rubygems.org'
      gem 'rake', '~> 13.0'
    GEMFILE

    directory 'lib' do
      file 'example.rb', content: <<~RUBY
        class Example
          def greet(name)
            "Hello, \#{name}!"
          end
        end
      RUBY
    end

    directory 'test' do
      file 'example_test.rb', content: '# Tests go here'
    end
  end
end

# Render for different LLM providers
puts context.render(:anthropic)
puts context.render(:openai)
puts context.render(:json)
```

## Core Concepts

### Context Builder

The `Builder` class provides a DSL for constructing contexts:

```ruby
context = Cosmos::Llm::Context.build do
  # Add structured blocks
  block :system, "You are an AI assistant"
  block :user, "Help me with this task"

  # Add string content
  string "Additional context information"

  # Load content from filesystem
  file_content '/path/to/file.rb', name: :source_code

  # Build virtual filesystem
  filesystem do
    directory 'src' do
      file 'main.rb', content: 'puts "Hello"'
    end
  end
end
```

### Blocks

Blocks are immutable containers for different types of content:

```ruby
# Create a block
block = Cosmos::Llm::Context::Block.new(:system, "You are helpful")

# Check block type
block.type?(:system)  # => true

# Access properties
block.name     # => :system
block.content  # => "You are helpful"

# Blocks are immutable - create new instances for changes
new_block = block.with_content("New content")
new_block_with_meta = block.with_metadata(role: 'assistant')
```

### Virtual Filesystem

Create hierarchical file structures:

```ruby
fs = Cosmos::Llm::Context::Filesystem.new('/') do
  # Root-level files
  file 'README.md', content: '# My Project'
  file '.gitignore', content: '*.log'

  # Nested directories
  directory 'src' do
    file 'main.rb', content: 'puts "Hello"'

    directory 'lib' do
      file 'helper.rb', content: 'def help; end'
    end
  end

  directory 'test' do
    file 'test_main.rb', content: 'require "minitest"'
  end
end

# Query filesystem
fs.find_file('src/main.rb')
fs.all_files
fs.tree  # Visual tree representation
```

### Renderers

Export contexts in multiple formats:

#### Default Renderer
```ruby
context.render(:default)
```

#### XML Renderer
```ruby
context.render(:xml)
```

#### JSON Renderer
```ruby
context.render(:json)
```

#### Anthropic Claude Format
```ruby
context.render(:anthropic)
```

#### OpenAI Format
```ruby
context.render(:openai)
```

### Custom Renderers

Register your own renderers:

```ruby
class MarkdownRenderer
  def self.render(builder)
    output = []
    output << "# Context\n"

    builder.blocks.each do |block|
      output << "\n## #{block.name.to_s.capitalize}\n"
      output << block.content.to_s
    end

    if builder.root_filesystem
      output << "\n## Files\n"
      output << builder.root_filesystem.tree
    end

    output.join("\n")
  end
end

# Register the custom renderer
Cosmos::Llm::Context::Renderers.register(:markdown, MarkdownRenderer)

# Use it
context.render(:markdown)
```

## Advanced Usage

### Immutable Design

All core objects (Blocks, VirtualFiles, Filesystems) are immutable by default:

```ruby
block = Cosmos::Llm::Context::Block.new(:system, "content")
# block.content = "new"  # Raises FrozenError

# Create new instances with changes
new_block = block.with_content("new content")
```

### Metadata and Attributes

Add metadata to blocks and files:

```ruby
# Block with metadata
block = Cosmos::Llm::Context::Block.new(:system, "content", role: 'assistant')
block.meta(:role)  # => 'assistant'

# VirtualFile with attributes
file = Cosmos::Llm::Context::VirtualFile.new('script.sh', 'echo hi', executable: true)
file.attributes  # => { executable: true }
```

### Content Loading

Load content from actual filesystem:

```ruby
context = Cosmos::Llm::Context.build do
  # Load a specific file
  file_content '/path/to/config.yml', name: :config

  # Multiple files
  file_content '/path/to/source.rb', name: :source
  file_content '/path/to/test.rb', name: :tests
end
```

### Type Safety and Validation

The library validates inputs and provides clear error messages:

```ruby
# Invalid block name
Cosmos::Llm::Context::Block.new(nil, "content")
# => raises InvalidNameError: Name cannot be nil

# Invalid filename
Cosmos::Llm::Context::VirtualFile.new("path/to/file", "content")
# => raises InvalidPathError: Filename cannot contain path separators

# Unknown renderer
context.render(:unknown)
# => raises RendererNotFoundError: Unknown renderer format: unknown
```

## API Reference

Full API documentation is available at [https://rubydoc.info/gems/cosmos-llm-context](https://rubydoc.info/gems/cosmos-llm-context)

### Main Classes

- **`Cosmos::Llm::Context`** - Main module and entry point
- **`Cosmos::Llm::Context::Builder`** - DSL for building contexts
- **`Cosmos::Llm::Context::Block`** - Content block container
- **`Cosmos::Llm::Context::Filesystem`** - Virtual filesystem
- **`Cosmos::Llm::Context::VirtualFile`** - Virtual file representation
- **`Cosmos::Llm::Context::Renderers`** - Renderer registration and management

### Exception Classes

- **`Cosmos::Llm::Context::Error`** - Base error class
- **`InvalidNameError`** - Invalid name provided
- **`InvalidPathError`** - Invalid path provided
- **`FileNotFoundError`** - File not found in virtual filesystem
- **`RendererNotFoundError`** - Unknown renderer format
- **`ValidationError`** - Validation failed
- **`DuplicateRegistrationError`** - Duplicate registration attempt

## Architecture

### Modular Design

The library uses a mixin-based architecture for extensibility:

```
lib/cosmos/llm/context/
├── builder.rb              # Main DSL builder
├── builder_mixins/         # Extensible functionality
│   └── content_methods.rb  # Content loading methods
├── block.rb                # Context blocks
├── filesystem.rb           # Virtual filesystem
├── renderers.rb            # Output renderers
├── errors.rb               # Exception classes
└── version.rb              # Version constant
```

### Extensibility

Add custom functionality via mixins:

```ruby
# lib/cosmos/llm/context/builder_mixins/my_feature.rb
module Cosmos::Llm::Context::BuilderMixins
  module MyFeature
    def my_custom_method
      # Custom functionality
    end
  end
end

# The mixin is automatically loaded and included
```

## Development

After checking out the repo, run:

```bash
$ bundle install
```

Run tests:

```bash
$ rake test
```

Run linter:

```bash
$ rubocop
```

## Contributing

We welcome contributions! To contribute:

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Make your changes with tests
4. Ensure tests pass (`rake test`)
5. Ensure code quality (`rubocop`)
6. Commit your changes (`git commit -m 'Add amazing feature'`)
7. Push to the branch (`git push origin feature/amazing-feature`)
8. Open a Pull Request

Please ensure:
- All tests pass
- Code follows RuboCop guidelines
- New features include tests and documentation
- Commit messages are clear and descriptive

## Philosophy

This library is built following Durable Programming principles:

- **Pragmatic Problem-Solving**: Solve real-world LLM context management challenges
- **Sustainability**: Design for long-term maintenance and evolution
- **Quality**: Comprehensive testing and documentation
- **Immutability**: Thread-safe, predictable objects
- **Modularity**: Extensible architecture via mixins and registration patterns

## Security

To report security vulnerabilities, please email security@cosmos-llm.com. Do not open public issues for security concerns.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Support

- **Documentation**: [https://rubydoc.info/gems/cosmos-llm-context](https://rubydoc.info/gems/cosmos-llm-context)
- **Issues**: [https://github.com/cosmos-llm/cosmos-llm-context/issues](https://github.com/cosmos-llm/cosmos-llm-context/issues)
- **Email**: commercial@cosmos-llm.com

## Credits

Created and maintained by [Durable Programming, LLC](https://cosmos-llm.com).

## Copyright

Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
