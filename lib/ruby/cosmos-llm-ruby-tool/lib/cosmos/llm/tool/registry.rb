# frozen_string_literal: true

module Cosmos
  module Llm
    module Tool
      # Registry for managing tool definitions.
      #
      # This class provides a centralized registry for storing and retrieving
      # tool definitions, enabling tool discovery and management.
      #
      # @example Using the registry
      #   registry = Registry.new
      #   registry.register(my_tool)
      #   tool = registry.get(:calculator)
      #
      # @see Definition
      class Registry
        # @return [Hash<Symbol, Definition>] The registered tools
        attr_reader :tools

        # Initializes a new Registry.
        #
        # @return [Registry] A new registry instance
        def initialize
          @tools = {}
        end

        # Registers a tool in the registry.
        #
        # @param tool [Definition] The tool to register
        # @return [Definition] The registered tool
        # @raise [ArgumentError] If tool is not a Definition
        # @example Register a tool
        #   registry.register(calculator_tool)
        def register(tool)
          raise ArgumentError, "Expected a Tool::Definition, got #{tool.class}" unless tool.is_a?(Definition)

          @tools[tool.name] = tool
        end

        # Retrieves a tool by name.
        #
        # @param name [Symbol, String] The tool name
        # @return [Definition, nil] The tool or nil if not found
        # @example Get a tool
        #   tool = registry.get(:calculator)
        def get(name)
          @tools[name&.to_sym]
        end

        # Checks if a tool is registered.
        #
        # @param name [Symbol, String] The tool name
        # @return [Boolean] True if the tool is registered
        # @example Check if tool exists
        #   registry.registered?(:calculator) # => true
        def registered?(name)
          @tools.key?(name&.to_sym)
        end

        # Removes a tool from the registry.
        #
        # @param name [Symbol, String] The tool name
        # @return [Definition, nil] The removed tool or nil
        # @example Unregister a tool
        #   registry.unregister(:calculator)
        def unregister(name)
          @tools.delete(name&.to_sym)
        end

        # Returns all registered tools.
        #
        # @return [Array<Definition>] Array of all tools
        # @example List all tools
        #   tools = registry.all
        def all
          @tools.values
        end

        # Returns all tool names.
        #
        # @return [Array<Symbol>] Array of tool names
        # @example List tool names
        #   names = registry.names # => [:calculator, :weather]
        def names
          @tools.keys
        end

        # Clears all tools from the registry.
        #
        # @return [void]
        # @example Clear all tools
        #   registry.clear
        def clear
          @tools.clear
        end

        # Returns the number of registered tools.
        #
        # @return [Integer] The count of tools
        # @example Get tool count
        #   registry.count # => 5
        def count
          @tools.size
        end

        alias size count

        # Yields each tool to the block.
        #
        # @yield [tool] Yields each tool
        # @yieldparam tool [Definition] A registered tool
        # @return [void]
        # @example Iterate over tools
        #   registry.each { |tool| puts tool.name }
        def each(&block)
          @tools.values.each(&block)
        end

        # Converts the registry to a hash.
        #
        # @return [Hash] Hash representation with tool count and names
        def to_h
          {
            count: count,
            tools: names
          }
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
