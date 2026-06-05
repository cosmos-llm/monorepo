# frozen_string_literal: true

require 'cosmos/llm/tool'

module Cosmos
  module Llm
    module Tool
      module Preset
        # Diff preset tool (virtual filesystem)
        #
        # Generates a unified diff between two files in the virtual filesystem,
        # or between a file and a provided string. The output is a standard
        # unified diff that LLMs can read to understand what changed, or produce
        # to request an edit via the patch tool.
        #
        # Uses Ruby's diff-lcs gem for pure-Ruby diff generation.
        #
        # @example Diff two files
        #   tool = Cosmos::Llm::Tool::Preset.diff(filesystem)
        #   result = tool.call(path_a: 'src/main.rb', path_b: 'src/main_new.rb')
        #
        # @example Diff a file against a string
        #   tool = Cosmos::Llm::Tool::Preset.diff(filesystem)
        #   result = tool.call(path_a: 'src/main.rb', content_b: 'puts "goodbye"')
        #
        # @param filesystem [Cosmos::Llm::VirtualFilesystem::Filesystem]
        # @return [Cosmos::Llm::Tool::Definition]
        def self.diff(filesystem)
          Cosmos::Llm::Tool.define(:diff, register: false) do
            description 'Generate a unified diff between two files or between a file and provided content'

            parameter :path_a,
                      type: :string,
                      required: true,
                      description: 'Path to the original file in the virtual filesystem'

            parameter :path_b,
                      type: :string,
                      required: false,
                      description: 'Path to the modified file in the virtual filesystem (mutually exclusive with content_b)'

            parameter :content_b,
                      type: :string,
                      required: false,
                      description: 'Modified content as a string (mutually exclusive with path_b)'

            parameter :context_lines,
                      type: :number,
                      required: false,
                      description: 'Number of context lines around each change (default: 3)'

            execute do |params|
              path_a     = params[:path_a]
              path_b     = params[:path_b]
              content_b  = params[:content_b]
              context    = params.fetch(:context_lines, 3).to_i

              if path_b.nil? && content_b.nil?
                next { success: false, error: 'Either path_b or content_b is required', path_a: path_a }
              end
              if path_b && content_b
                next { success: false, error: 'path_b and content_b are mutually exclusive', path_a: path_a }
              end

              begin
                file_a = filesystem.find_file(path_a)
                unless file_a
                  next { success: false, error: 'File not found in virtual filesystem', path: path_a }
                end

                text_a = file_a.content || ''

                if path_b
                  file_b = filesystem.find_file(path_b)
                  unless file_b
                    next { success: false, error: 'File not found in virtual filesystem', path: path_b }
                  end
                  text_b     = file_b.content || ''
                  label_b    = path_b
                else
                  text_b  = content_b
                  label_b = "#{path_a} (new)"
                end

                diff_text = Preset::Diff.unified_diff(text_a, text_b, path_a, label_b, context)

                {
                  success: true,
                  path_a: path_a,
                  path_b: label_b,
                  diff: diff_text,
                  changed: !diff_text.empty?
                }
              rescue StandardError => e
                { success: false, error: e.message, path_a: path_a }
              end
            end
          end
        end

        # Pure-Ruby unified diff generator.
        module Diff
          # Generates a unified diff string from two texts.
          #
          # @param text_a [String]
          # @param text_b [String]
          # @param label_a [String]
          # @param label_b [String]
          # @param context [Integer] lines of context around each hunk
          # @return [String] unified diff, empty string if identical
          def self.unified_diff(text_a, text_b, label_a, label_b, context = 3)
            lines_a = text_a.lines
            lines_b = text_b.lines

            hunks = build_hunks(lines_a, lines_b, context)
            return '' if hunks.empty?

            header = "--- #{label_a}\n+++ #{label_b}\n"
            header + hunks.map { |h| format_hunk(h) }.join
          end

          # @api private
          def self.build_hunks(lines_a, lines_b, context)
            edits = lcs_diff(lines_a, lines_b)
            return [] if edits.empty?

            # Group edits into hunks separated by more than 2*context unchanged lines.
            groups = []
            current = []
            edits.each do |edit|
              if current.empty? || edit[:ia] - current.last[:ia] <= context * 2 + 1 || edit[:ia].nil? || current.last[:ia].nil?
                current << edit
              else
                groups << current
                current = [edit]
              end
            end
            groups << current unless current.empty?
            groups
          end

          # Produce a list of changed positions. Each entry: { type: :del/:ins/:chg, ia:, ib:, old:, new: }
          # Uses a simple Myers-like LCS approach via array comparison.
          # @api private
          def self.lcs_diff(lines_a, lines_b)
            # Build LCS table
            m = lines_a.length
            n = lines_b.length
            lcs = Array.new(m + 1) { Array.new(n + 1, 0) }

            (1..m).each do |i|
              (1..n).each do |j|
                lcs[i][j] = if lines_a[i - 1] == lines_b[j - 1]
                               lcs[i - 1][j - 1] + 1
                             else
                               [lcs[i - 1][j], lcs[i][j - 1]].max
                             end
              end
            end

            # Backtrack to produce edits
            edits = []
            i = m
            j = n
            while i > 0 || j > 0
              if i > 0 && j > 0 && lines_a[i - 1] == lines_b[j - 1]
                i -= 1
                j -= 1
              elsif j > 0 && (i == 0 || lcs[i][j - 1] >= lcs[i - 1][j])
                edits.unshift({ type: :ins, ia: i, ib: j - 1, line: lines_b[j - 1] })
                j -= 1
              else
                edits.unshift({ type: :del, ia: i - 1, ib: j, line: lines_a[i - 1] })
                i -= 1
              end
            end
            edits
          end

          # @api private
          def self.format_hunk(edits)
            return '' if edits.empty?

            lines_out = []
            edits.each do |edit|
              case edit[:type]
              when :del
                lines_out << "-#{edit[:line]}"
              when :ins
                lines_out << "+#{edit[:line]}"
              end
            end

            # Compute hunk header counts
            del_count = edits.count { |e| e[:type] == :del }
            ins_count = edits.count { |e| e[:type] == :ins }
            start_a   = edits.find { |e| e[:ia] }&.dig(:ia).to_i + 1
            start_b   = edits.find { |e| e[:ib] }&.dig(:ib).to_i + 1

            "@@ -#{start_a},#{del_count} +#{start_b},#{ins_count} @@\n#{lines_out.join}\n"
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
