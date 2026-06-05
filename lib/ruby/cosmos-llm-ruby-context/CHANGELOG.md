# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Comprehensive architecture improvements based on Durable Programming coding standards
- Custom exception hierarchy for better error handling
  - `Error` - Base exception class
  - `InvalidNameError` - Invalid name provided
  - `InvalidPathError` - Invalid path provided
  - `FileNotFoundError` - File not found in virtual filesystem
  - `RendererNotFoundError` - Unknown renderer format
  - `ValidationError` - Validation failed
  - `DuplicateRegistrationError` - Duplicate registration attempt
- Modular mixin architecture for Builder extensibility
  - `BuilderMixins::ContentMethods` - Content loading functionality
  - Dynamic mixin loading system
- Renderer registration system
  - `Renderers.register` - Register custom renderers
  - `Renderers.available_formats` - List available renderers
  - `Renderers.registered?` - Check if format is registered
  - Lazy loading for built-in renderers
- Immutability for core objects
  - Block objects are now frozen and immutable
  - VirtualFile objects are now frozen and immutable
  - `Block#with_content` - Create new block with updated content
  - `Block#with_metadata` - Create new block with updated metadata
  - `VirtualFile#with_content` - Create new file with updated content
  - `VirtualFile#with_attributes` - Create new file with updated attributes
- Comprehensive input validation
  - Block name validation and coercion
  - VirtualFile filename validation (prevents path separators, null bytes)
  - Metadata and attributes validation
- Equality and hash methods for value objects
  - `Block#==`, `Block#eql?`, `Block#hash`
  - `VirtualFile#==`, `VirtualFile#eql?`, `VirtualFile#hash`
  - `Filesystem#==`, `Filesystem#eql?`, `Filesystem#hash`
- Comprehensive YARD documentation with examples
- Detailed README with quick start, examples, and API reference
- CHANGELOG following Keep a Changelog format

### Changed
- Builder content methods moved to `BuilderMixins::ContentMethods`
- `Block#content` changed from `attr_accessor` to `attr_reader` (immutable)
- `Block#meta` changed from getter/setter to getter-only
- `VirtualFile#content` changed from `attr_accessor` to `attr_reader` (immutable)
- Renderer error changed from `ArgumentError` to `RendererNotFoundError`
- File loading validation improved with better error messages

### Fixed
- Thread safety through immutable objects
- Consistent error handling across all classes

## [0.1.0] - 2025-01-XX (Initial Release)

### Added
- Core DSL for building LLM contexts
- Virtual filesystem support with hierarchical directory structures
- Context block system for organizing content
- Multiple output renderers
  - Default text renderer
  - XML renderer
  - JSON renderer
  - Anthropic Claude-specific renderer
  - OpenAI-specific renderer
- Builder DSL with block evaluation
- Zeitwerk autoloading integration
- Comprehensive test suite with Minitest
- Development dependencies
  - RuboCop for code style
  - YARD for documentation
  - Mocha for test mocking
- Basic YARD documentation
- Example usage file

[Unreleased]: https://github.com/cosmos-llm/cosmos-llm-context/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/cosmos-llm/cosmos-llm-context/releases/tag/v0.1.0
