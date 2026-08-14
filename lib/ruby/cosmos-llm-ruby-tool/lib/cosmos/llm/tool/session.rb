# frozen_string_literal: true

module Cosmos
  module Llm
    module Tool
      # Per-run mutable state shared by a loop and the tools it dispatches:
      # named call budgets, a finish flag, and free-form notes.
      #
      # A tool handler closes over a Session and consults it before doing
      # anything expensive. The caps live here rather than in a prompt because
      # a prompt is a request and this is an invariant — a model asked nicely
      # to stop searching will, eventually, search again.
      #
      # ## Budgets are named, not enumerated
      #
      # The gem cannot know what a given agent spends. Rather than hardcode
      # counters for searches and file opens, a Session takes a hash of caps
      # keyed by whatever the caller wants to meter:
      #
      #   session = Session.new(budgets: { search: 40, open: 25 })
      #   session.allowed?(:search)   # => true
      #   session.consume(:search)    # => 1
      #
      # An unlisted key is unmetered, so a tool that calls +consume+ on
      # something the caller never capped still records the count and always
      # reports +allowed?+.
      #
      # ## Exhaustion is a message, not an exception
      #
      # {#exhausted_message} exists because the right response to a spent
      # budget is to tell the model, in the tool result, that it has run out
      # and should wrap up. Raising instead would abort a run that is mostly
      # successful and discard whatever the agent had already gathered.
      #
      # @example A tool handler that respects its budget
      #   search = Definition.new(:search) do
      #     description 'Search the corpus'
      #     parameter :query, type: :string, required: true
      #     execute do |params|
      #       next session.exhausted_message(:search) unless session.allowed?(:search)
      #
      #       session.consume(:search)
      #       backend.search(params[:query])
      #     end
      #   end
      class Session
        # @return [Hash{Symbol=>Integer}] the cap for each metered key
        attr_reader :budgets

        # @return [Hash{Symbol=>Integer}] how many times each key was consumed
        attr_reader :counts

        # @return [Array<String>] notes left by tools during the run
        attr_reader :notes

        # @param budgets [Hash{Symbol,String=>Integer}] cap per named key. Keys
        #   absent from this hash are counted but never exhausted.
        def initialize(budgets: {})
          @budgets = budgets.to_h { |k, v| [k.to_sym, v.to_i] }
          @counts = Hash.new(0)
          @notes = []
          @finished = false
        end

        # Whether another call against +key+ is within budget.
        #
        # @param key [Symbol, String] the metered key
        # @return [Boolean] true when unmetered or still under the cap
        def allowed?(key)
          cap = @budgets[key.to_sym]
          cap.nil? || @counts[key.to_sym] < cap
        end

        # Records one use of +key+, whether or not it was within budget.
        #
        # Counting past the cap is deliberate: a run summary that says an agent
        # tried to search 60 times against a cap of 40 is more useful than one
        # that stops counting at 40.
        #
        # @param key [Symbol, String] the metered key
        # @param n [Integer] how many uses to record
        # @return [Integer] the new count for that key
        def consume(key, n = 1)
          @counts[key.to_sym] += n
        end

        # How many uses of +key+ remain.
        #
        # @param key [Symbol, String] the metered key
        # @return [Integer, nil] remaining uses, or nil when unmetered
        def remaining(key)
          cap = @budgets[key.to_sym]
          return nil if cap.nil?

          [cap - @counts[key.to_sym], 0].max
        end

        # The text to hand back to the model when a budget is spent.
        #
        # @param key [Symbol, String] the exhausted key
        # @return [String] a tool result telling the model to wrap up
        def exhausted_message(key)
          "#{key} budget exhausted (#{@counts[key.to_sym]} used). " \
            'Record what you have and finish.'
        end

        # Whether a tool signalled that the run is over.
        #
        # @return [Boolean] true once {#finish!} has been called
        def finished?
          @finished
        end

        # Signals that the run is over. A +done+ tool calls this; the loop
        # checks it after each turn and stops.
        #
        # @param note [String, nil] a closing summary to record
        # @return [void]
        def finish!(note = nil)
          @notes << note.to_s unless note.nil? || note.to_s.empty?
          @finished = true
        end

        # Records a note without ending the run.
        #
        # @param note [String] the note
        # @return [void]
        def note(note)
          @notes << note.to_s
        end

        # A summary of the run, for logging.
        #
        # @return [Hash] +:counts+, +:budgets+, +:finished+, +:notes+
        def to_h
          {
            counts: @counts.dup,
            budgets: @budgets.dup,
            finished: @finished,
            notes: @notes.dup
          }
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
