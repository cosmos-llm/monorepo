# frozen_string_literal: true

require "optparse"
require_relative "chat_session"
require_relative "error"

module CosmosLlmHello
  # Entry point for the interactive CLI chatbot.
  class CLI
    QUIT_COMMANDS = %w[exit quit q].freeze
    RESET_COMMAND = "/reset"
    HELP_TEXT = <<~HELP
      Commands:
        /reset   — clear conversation history
        exit     — quit (also: quit, q)
    HELP

    # Parse ARGV and run the chatbot loop.
    #
    # @param argv [Array<String>] command-line arguments
    # @return [Integer] exit code
    def self.start(argv = ARGV)
      new(argv).run
    end

    # @param argv [Array<String>] command-line arguments
    def initialize(argv = [])
      @options = parse_options(argv)
    end

    # Run the interactive REPL until the user exits.
    #
    # @return [Integer] exit code (0 on normal exit)
    def run
      session = ChatSession.new(
        provider: @options[:provider],
        model: @options[:model],
        system_prompt: @options[:system_prompt]
      )

      puts "cosmos-llm-ruby-demo-hello — #{@options[:provider]} / #{@options[:model]}"
      puts "Type 'exit' to quit, '/reset' to clear history."
      puts

      loop do
        print "You: "
        input = $stdin.gets
        break unless input

        input = input.chomp.strip
        next if input.empty?

        if QUIT_COMMANDS.include?(input.downcase)
          puts "Goodbye."
          break
        end

        if input == RESET_COMMAND
          session.reset
          puts "(conversation reset)"
          next
        end

        begin
          reply = session.chat(input)
          puts "Assistant: #{reply}"
          puts
        rescue CosmosLlmHello::ProviderError => e
          warn "Error: #{e.message}"
        end
      end

      0
    end

    private

    def parse_options(argv)
      options = {
        provider: ChatSession::DEFAULT_PROVIDER,
        model: ChatSession::DEFAULT_MODEL,
        system_prompt: nil
      }

      OptionParser.new do |opts|
        opts.banner = "Usage: cosmos-llm-ruby-demo-hello [options]"

        opts.on("-p", "--provider PROVIDER", "LLM provider (default: #{ChatSession::DEFAULT_PROVIDER})") do |v|
          options[:provider] = v
        end

        opts.on("-m", "--model MODEL", "Model name (default: #{ChatSession::DEFAULT_MODEL})") do |v|
          options[:model] = v
        end

        opts.on("-s", "--system PROMPT", "System prompt") do |v|
          options[:system_prompt] = v
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
