# frozen_string_literal: true

require "pathname"

module CosmosLlmMarkdowner
  # Enforces read/write permissions for the agent.
  #
  # The agent may not touch the real filesystem at all unless the user
  # explicitly grants access via --in, --out, or --out-dir flags. Every
  # path the agent tries to read or write is checked here first.
  class Sandbox
    # @param allowed_reads  [Array<String>] absolute paths the agent may read
    # @param allowed_writes [Array<String>] absolute paths the agent may write
    # @param allowed_dirs   [Array<String>] absolute directory paths the agent may write into
    def initialize(allowed_reads: [], allowed_writes: [], allowed_dirs: [])
      @allowed_reads  = allowed_reads.map  { |p| Pathname.new(p).expand_path }
      @allowed_writes = allowed_writes.map { |p| Pathname.new(p).expand_path }
      @allowed_dirs   = allowed_dirs.map   { |p| Pathname.new(p).expand_path }
    end

    # Read a file the user allowed via --in.
    #
    # @param path [String] path to read
    # @return [String] file contents
    # @raise [CosmosLlmMarkdowner::FilesystemError] if path not in allowed reads
    def read(path)
      abs = Pathname.new(path).expand_path
      raise FilesystemError, "Read denied: #{abs} — use --in to allow" unless readable?(abs)

      abs.read
    end

    # Write content to a path the user allowed via --out or --out-dir.
    #
    # @param path    [String] destination path
    # @param content [String] content to write
    # @return [void]
    # @raise [CosmosLlmMarkdowner::FilesystemError] if path not in allowed writes
    def write(path, content)
      abs = Pathname.new(path).expand_path
      raise FilesystemError, "Write denied: #{abs} — use --out or --out-dir to allow" unless writable?(abs)

      abs.dirname.mkpath
      abs.write(content)
    end

    # @param path [Pathname] expanded path
    # @return [Boolean]
    def readable?(path)
      @allowed_reads.any? { |r| r == path }
    end

    # @param path [Pathname] expanded path
    # @return [Boolean]
    def writable?(path)
      return true if @allowed_writes.any? { |w| w == path }
      return true if @allowed_dirs.any?   { |d| path.to_s.start_with?("#{d}/") || path.dirname == d }

      false
    end

    # @return [Array<Pathname>] files the agent is allowed to read
    def readable_paths
      @allowed_reads.dup
    end

    # @return [Array<Pathname>] explicit output files
    def writable_files
      @allowed_writes.dup
    end

    # @return [Array<Pathname>] output directories
    def writable_dirs
      @allowed_dirs.dup
    end
  end
end
