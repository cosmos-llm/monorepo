# frozen_string_literal: true

require "optparse"
require_relative "sandbox"
require_relative "agent_session"
require_relative "error"

module CosmosLlmMarkdowner
  # Entry point for the cosmos-llm-ruby-demo-markdowner CLI.
  class CLI
    # Parse ARGV and run.
    #
    # @param argv [Array<String>] command-line arguments
    # @return [Integer] exit code
    def self.start(argv = ARGV)
      new(argv).run
    end

    # @param argv [Array<String>] command-line arguments
    def initialize(argv = [])
      @options = parse_options(argv)
      @prompt  = @options.delete(:prompt)
    end

    # Run interactively (or with a one-shot --prompt).
    #
    # @return [Integer] exit code
    def run
      sandbox = Sandbox.new(
        allowed_reads: @options[:in_files],
        allowed_writes: @options[:out_files],
        allowed_dirs: @options[:out_dirs]
      )

      session = AgentSession.new(
        sandbox: sandbox,
        provider: @options[:provider],
        model: @options[:model],
        preload: @options[:in_files]
      )

      if @prompt
        reply = session.run(@prompt)
        puts reply
        return 0
      end

      puts "cosmos-llm-ruby-demo-markdowner — #{@options[:provider]} / #{@options[:model]}"
      puts "Type your request. Enter a blank line to submit. 'exit' to quit."
      print_permissions(sandbox)
      puts

      loop do
        input = read_multiline_input
        break if input.nil?

        input = input.strip
        next if input.empty?
        break if %w[exit quit q].include?(input.downcase)

        begin
          reply = session.run(input)
          puts
          puts reply
          puts
        rescue CosmosLlmMarkdowner::ProviderError => e
          warn "Error: #{e.message}"
        end
      end

      0
    end

    private

    def read_multiline_input
      print "You: "
      lines = []
      loop do
        line = $stdin.gets
        return nil if line.nil?

        line = line.chomp
        break if line.empty? && !lines.empty?

        lines << line
      end
      lines.join("\n")
    end

    def print_permissions(sandbox)
      if sandbox.readable_paths.any?
        puts "  reads:  #{sandbox.readable_paths.join(", ")}"
      else
        puts "  reads:  none (use --in to allow)"
      end

      writable = sandbox.writable_files.map(&:to_s) + sandbox.writable_dirs.map { |d| "#{d}/*" }
      if writable.any?
        puts "  writes: #{writable.join(", ")}"
      else
        puts "  writes: none (use --out or --out-dir to allow)"
      end
    end

    def parse_options(argv)
      options = {
        provider: AgentSession::DEFAULT_PROVIDER,
        model: AgentSession::DEFAULT_MODEL,
        in_files: [],
        out_files: [],
        out_dirs: [],
        prompt: nil
      }

      OptionParser.new do |opts|
        opts.banner = "Usage: cosmos-llm-ruby-demo-markdowner [options] [--prompt TEXT]"

        opts.on("-p", "--provider PROVIDER",
                "LLM provider (default: #{AgentSession::DEFAULT_PROVIDER})") do |v|
          options[:provider] = v
        end

        opts.on("-m", "--model MODEL",
                "Model name (default: #{AgentSession::DEFAULT_MODEL})") do |v|
          options[:model] = v
        end

        opts.on("--in FILE",
                "Allow agent to read FILE (may be specified multiple times)") do |v|
          options[:in_files] << File.expand_path(v)
        end

        opts.on("--out FILE",
                "Allow agent to write FILE (may be specified multiple times)") do |v|
          options[:out_files] << File.expand_path(v)
        end

        opts.on("--out-dir DIR",
                "Allow agent to write any file inside DIR (may be specified multiple times)") do |v|
          options[:out_dirs] << File.expand_path(v)
        end

        opts.on("--prompt TEXT", "Run a single prompt non-interactively and exit") do |v|
          options[:prompt] = v
        end

        opts.on("-h", "--help", "Show this help") do
          puts opts
          exit 0
        end
      end.parse!(argv)

      options
    end
  end
end
