# frozen_string_literal: true

module Cosmos
  module Llm
    module Evaluate
      # Verifies that quoted evidence actually appears in its source document.
      #
      # When a model is asked to extract claims and quote the text supporting
      # each one, checking those quotes mechanically is what separates an
      # extraction from a plausible-sounding invention. A model that cannot
      # find support tends to write something that reads like support.
      #
      # This is deliberately not a similarity score. A quote either appears in
      # the source or it does not; a claim assembled out of words scattered
      # through the document is exactly the failure this catches, so the check
      # stays a contiguous substring match.
      #
      # ## What normalization forgives, and what it does not
      #
      # Markdown conversion, PDF extraction, and HTML-to-text all rewrite
      # punctuation and line wrapping. A quote differing from its source only
      # in curly quotes, dash width, or where a line happened to break is
      # verbatim in every way that matters, so {normalize} folds those.
      #
      # Case is folded for the same reason: a model copying a sentence from
      # mid-paragraph routinely lowercases the leading word. Folding case does
      # not weaken the check the way loosening it to token matching would —
      # the quote must still appear contiguously.
      #
      # @example Check one quote
      #   Evidence.present?('conversion rate rose 7%', document_text)  # => true
      #
      # @example Report every unsupported quote in a nested result
      #   Evidence.unverified(finding, document_text)
      #   # => ["results[0].evidence: quote not found in source: \"...\""]
      module Evidence
        # Quote characters rewritten to a plain apostrophe.
        SINGLE_QUOTES = "‘’‛′"

        # Quote characters rewritten to a plain double quote.
        DOUBLE_QUOTES = '“”‟″'

        # Dash characters rewritten to a plain hyphen.
        DASHES = '–—−'

        module_function

        # Collapses whitespace and normalizes the punctuation that text
        # conversion tends to rewrite.
        #
        # @param str [String] raw text
        # @return [String] normalized text
        def normalize(str)
          str.to_s
             .unicode_normalize(:nfkc)
             .tr(SINGLE_QUOTES, "'")
             .tr(DOUBLE_QUOTES, '"')
             .tr(DASHES, '-')
             .gsub(/\s+/, ' ')
             .strip
        end

        # Whether a quote appears in the given text.
        #
        # An empty quote is never present: a model that omits its evidence has
        # not supported anything, and treating "" as trivially found would
        # verify every claim that skipped the field.
        #
        # @param quote [String] the claimed verbatim quote
        # @param text [String] the source document text
        # @return [Boolean] true when the normalized quote is a substring
        def present?(quote, text)
          needle = normalize(quote)
          return false if needle.empty?

          normalize(text).downcase.include?(needle.downcase)
        end

        # Walks a structure and reports every +evidence+ value that cannot be
        # found in the source.
        #
        # @param record [Hash, Array] a result containing +evidence+ keys
        # @param text [String] the source document text
        # @param key [String] the field name holding quotes
        # @return [Array<String>] one message per unsupported quote, empty when
        #   all verify
        def unverified(record, text, key: 'evidence')
          collect(record, key: key).filter_map do |path, quote|
            next if present?(quote, text)

            "#{path}: quote not found in source: #{quote.to_s[0, 80].inspect}"
          end
        end

        # Whether every quote in a structure is supported by the source.
        #
        # @param record [Hash, Array] a result containing +evidence+ keys
        # @param text [String] the source document text
        # @param key [String] the field name holding quotes
        # @return [Boolean] true when nothing is unverified
        def verified?(record, text, key: 'evidence')
          unverified(record, text, key: key).empty?
        end

        # Gathers every quote in a nested structure alongside its path.
        #
        # Both string and symbol keys are matched, since a result parsed from
        # JSON and one built in Ruby should behave the same.
        #
        # @param node [Object] hash, array, or scalar
        # @param path [String] accumulated path prefix
        # @param key [String] the field name holding quotes
        # @return [Array<Array(String, String)>] +[path, quote]+ pairs
        def collect(node, path = '', key: 'evidence')
          case node
          when Hash
            node.flat_map do |k, v|
              child = path.empty? ? k.to_s : "#{path}.#{k}"
              k.to_s == key.to_s && v.is_a?(String) ? [[child, v]] : collect(v, child, key: key)
            end
          when Array
            node.each_with_index.flat_map { |v, i| collect(v, "#{path}[#{i}]", key: key) }
          else
            []
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
