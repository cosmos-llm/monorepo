# frozen_string_literal: true

require 'json'

module Cosmos
  module Llm
    module Predict
      # Splits a reply containing several top-level JSON objects into
      # individual objects.
      #
      # Extraction work usually asks for one object per finding rather than one
      # object holding an array, because a model that must close an array
      # tends to pad it to look complete, and a truncated array is unparseable
      # where a truncated stream is merely short.
      #
      # {Adapters::JsonAdapter} handles the single-object case, and its
      # first-brace-to-last-brace span is deliberately cheap. That span is
      # exactly wrong here: given two objects it returns both plus the text
      # between them, which parses as nothing.
      #
      # ## Why not split on blank lines
      #
      # Because quoted evidence breaks it. An extracted quote is a verbatim
      # excerpt from a source document, and those routinely span a paragraph
      # break, so a quote can itself contain "\n\n" and would be torn in half.
      #
      # Instead the reply is scanned character by character, tracking brace
      # depth and whether the cursor sits inside a string literal. A top-level
      # object ends when depth returns to zero. Whitespace between objects is
      # then irrelevant, which also means this tolerates objects run together
      # on one line, separated by commas, or wrapped in prose.
      #
      # @example Parse a reply holding one object per finding
      #   result = JsonStream.parse_all(reply)
      #   result[:objects].each { |o| puts o['value'] }
      #   warn result[:errors] if result[:errors].any?
      module JsonStream
        # A fence wrapping the entire reply.
        FENCE_HEAD = /\A```(?:json|jsonl|ndjson)?[ \t]*\r?\n?/i.freeze

        # The closing fence at the end of a reply.
        FENCE_TAIL = /\r?\n?```\s*\z/.freeze

        module_function

        # Scans text for balanced top-level JSON objects.
        #
        # Braces and newlines inside string literals are ignored, and a quote
        # preceded by a backslash does not close a string. Anything outside an
        # object — prose, a markdown fence, a stray comma — is skipped.
        #
        # @param text [String] the raw model reply
        # @return [Array<String>] source of each top-level object, in order
        def split(text)
          objects = []
          depth = 0
          start = nil
          in_string = false
          escaped = false

          text.to_s.each_char.with_index do |char, i|
            if in_string
              if escaped
                escaped = false
              elsif char == '\\'
                escaped = true
              elsif char == '"'
                in_string = false
              end
              next
            end

            case char
            when '"'
              in_string = true
            when '{'
              start = i if depth.zero?
              depth += 1
            when '}'
              next if depth.zero? # stray closer, e.g. a truncated reply

              depth -= 1
              if depth.zero? && start
                objects << text[start..i]
                start = nil
              end
            end
          end

          objects
        end

        # Splits a reply and parses each object.
        #
        # Objects are parsed independently, so one malformed object does not
        # discard the findings around it. That matters most when a reply is
        # truncated mid-object: everything before the truncation is still
        # usable, and the caller learns how much was lost.
        #
        # @param text [String] the raw model reply
        # @return [Hash] +:objects+ — successfully parsed hashes; +:errors+ —
        #   one message per object that failed to parse
        def parse_all(text)
          objects = []
          errors = []

          split(strip_fences(text)).each_with_index do |src, i|
            parsed = JSON.parse(src)
            if parsed.is_a?(Hash)
              objects << parsed
            else
              errors << "object #{i} was #{parsed.class}, expected an object"
            end
          rescue JSON::ParserError => e
            errors << "object #{i} did not parse: #{e.message}"
          end

          { objects: objects, errors: errors }
        end

        # Removes a markdown fence wrapping the whole reply.
        #
        # Fences between objects are left alone; the brace scan skips them.
        #
        # @param text [String] the raw reply
        # @return [String] the reply without a surrounding fence
        def strip_fences(text)
          text.to_s.strip.sub(FENCE_HEAD, '').sub(FENCE_TAIL, '')
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
