# frozen_string_literal: true

require 'digest'
require 'fileutils'
require 'json'

module Cosmos
  module Llm
    module Predict
      # Two-level cache for completion requests: memory, then disk.
      #
      # Evaluation loops and optimizers replay identical calls constantly, so a
      # cache is the difference between a devset sweep costing money every run
      # and costing money once. Lookups check an in-process LRU first, then a
      # directory of JSON files keyed by a digest of the request.
      #
      # Entries are stored as JSON rather than a marshalled object graph. That
      # rules out loading arbitrary Ruby objects from disk, which matters because
      # a cache directory is a file a later process trusts.
      #
      # @example
      #   cache = Cache.new(directory: '~/.cache/cosmos-llm')
      #   cache.fetch(provider: 'anthropic', model: 'claude-opus-4', messages: msgs) do
      #     client.completion(messages: msgs)
      #   end
      class Cache
        # Default cap on in-memory entries.
        DEFAULT_MAX_MEMORY_ENTRIES = 5_000

        # @return [String, nil] the on-disk cache directory, if enabled
        attr_reader :directory

        # Builds a cache.
        #
        # @param directory [String, nil] where to store entries on disk; when nil,
        #   the cache is memory-only and vanishes with the process
        # @param memory [Boolean] whether to keep an in-process LRU
        # @param max_memory_entries [Integer] cap on in-memory entries
        def initialize(directory: nil, memory: true, max_memory_entries: DEFAULT_MAX_MEMORY_ENTRIES)
          @directory = directory && File.expand_path(directory)
          @memory_enabled = memory
          @max_memory_entries = max_memory_entries
          @memory = {}
          @mutex = ::Mutex.new
          FileUtils.mkdir_p(@directory) if @directory
        end

        # Returns the cached value for a request, computing it on a miss.
        #
        # @param request [Hash] the request description, used to build the key
        # @yield computes the value when there is no cached entry
        # @return [Object] the cached or freshly computed value
        # @raise [ArgumentError] if no block is given
        # @example
        #   cache.fetch(model: 'x', messages: msgs) { client.completion(...) }
        def fetch(**request)
          raise ArgumentError, 'Cache#fetch requires a block' unless block_given?

          key = self.class.key_for(**request)
          hit = read(key)
          return hit[:value] if hit

          value = yield
          write(key, value)
          value
        end

        # Reads an entry without computing on a miss.
        #
        # @param key [String] the cache key
        # @return [Hash, nil] +{ value: ... }+ on a hit, nil on a miss
        def read(key)
          if @memory_enabled
            hit = @mutex.synchronize { touch(key) }
            return { value: hit } if hit
          end

          value = read_disk(key)
          return nil if value.nil?

          store_memory(key, value) if @memory_enabled
          { value: value }
        end

        # Writes an entry to both levels.
        #
        # @param key [String] the cache key
        # @param value [Object] a JSON-serializable value
        # @return [Object] the value
        def write(key, value)
          store_memory(key, value) if @memory_enabled
          write_disk(key, value)
          value
        end

        # Empties both cache levels.
        #
        # @return [void]
        def clear
          @mutex.synchronize { @memory.clear }
          FileUtils.rm_rf(@directory) if @directory && Dir.exist?(@directory)
          FileUtils.mkdir_p(@directory) if @directory
        end

        # @return [Integer] number of entries held in memory
        def size
          @mutex.synchronize { @memory.size }
        end

        # Builds a stable cache key from a request description.
        #
        # Keys are sorted before hashing so that hash ordering does not change
        # the digest, and the whole structure is serialized as JSON so that only
        # data — never object identity — contributes.
        #
        # @param request [Hash] the request description
        # @return [String] a hex digest
        # @example
        #   Cache.key_for(model: 'x', messages: [])
        def self.key_for(**request)
          Digest::SHA256.hexdigest(JSON.generate(normalize(request)))
        end

        # Recursively sorts hashes and stringifies keys for stable serialization.
        #
        # @param value [Object] any JSON-serializable structure
        # @return [Object] the normalized structure
        def self.normalize(value)
          case value
          when Hash
            value.map { |key, inner| [key.to_s, normalize(inner)] }.sort_by(&:first).to_h
          when Array
            value.map { |inner| normalize(inner) }
          when Symbol
            value.to_s
          else
            value
          end
        end

        private

        # Moves a key to the most-recent position and returns its value.
        #
        # Ruby hashes preserve insertion order, so delete-then-reinsert is all an
        # LRU needs here.
        #
        # @param key [String] the cache key
        # @return [Object, nil] the value, or nil on a miss
        def touch(key)
          return nil unless @memory.key?(key)

          value = @memory.delete(key)
          @memory[key] = value
          value
        end

        # @param key [String] the cache key
        # @param value [Object] the value to store
        # @return [void]
        def store_memory(key, value)
          @mutex.synchronize do
            @memory.delete(key)
            @memory[key] = value
            @memory.delete(@memory.first.first) while @memory.size > @max_memory_entries
          end
        end

        # @param key [String] the cache key
        # @return [Object, nil] the stored value, or nil if absent or unreadable
        def read_disk(key)
          return nil unless @directory

          path = path_for(key)
          return nil unless File.exist?(path)

          JSON.parse(File.read(path))['value']
        rescue JSON::ParserError, SystemCallError
          nil
        end

        # Writes an entry to disk, ignoring values that are not JSON-serializable.
        #
        # @param key [String] the cache key
        # @param value [Object] the value to store
        # @return [void]
        def write_disk(key, value)
          return unless @directory

          payload = JSON.generate({ 'key' => key, 'value' => value })
          path = path_for(key)
          FileUtils.mkdir_p(File.dirname(path))
          temp = "#{path}.#{Process.pid}.tmp"
          File.write(temp, payload)
          File.rename(temp, path)
        rescue JSON::GeneratorError, SystemCallError
          nil
        end

        # Shards entries across subdirectories by key prefix, so a large cache
        # does not put a hundred thousand files in one directory.
        #
        # @param key [String] the cache key
        # @return [String] the file path for that key
        def path_for(key)
          File.join(@directory, key[0, 2], "#{key}.json")
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
