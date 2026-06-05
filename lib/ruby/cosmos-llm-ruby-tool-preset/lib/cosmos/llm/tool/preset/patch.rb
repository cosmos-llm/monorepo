# frozen_string_literal: true

require 'cosmos/llm/tool'

module Cosmos
  module Llm
    module Tool
      module Preset
        # Patch preset tool (virtual filesystem)
        #
        # Applies a unified diff to a file in the virtual filesystem. Returns the
        # patched content so the caller can commit it back with the write tool, or
        # returns an error if the diff does not apply cleanly.
        #
        # Supports both interaction patterns:
        # - LLM-produced diffs: model outputs a unified diff string, patch applies it
        # - Snapshot history: caller generates diffs via the diff tool and stores them
        #
        # The tool does NOT mutate the virtual filesystem itself — it returns the
        # patched content so the caller can decide whether to commit it.
        #
        # @example Apply an LLM-produced diff
        #   tool = Cosmos::Llm::Tool::Preset.patch(filesystem)
        #   result = tool.call(file_path: 'src/main.rb', diff: diff_text)
        #   # result[:patched_content] => new file content if successful
        #
        # @param filesystem [Cosmos::Llm::VirtualFilesystem::Filesystem]
        # @return [Cosmos::Llm::Tool::Definition]
        def self.patch(filesystem)
          Cosmos::Llm::Tool.define(:patch, register: false) do
            description 'Apply a unified diff to a file in the virtual filesystem, returning the patched content'

            parameter :file_path,
                      type: :string,
                      required: true,
                      description: 'Path to the file to patch in the virtual filesystem'

            parameter :diff,
                      type: :string,
                      required: true,
                      description: 'Unified diff string to apply (as produced by the diff tool or by the LLM)'

            execute do |params|
              file_path = params[:file_path]
              diff_text = params[:diff]

              if file_path.nil? || !file_path.is_a?(String) || file_path.empty?
                next { success: false, error: 'file_path is required and must be a non-empty string' }
              end
              unless diff_text.is_a?(String) && !diff_text.empty?
                next { success: false, error: 'diff is required and must be a non-empty string', file_path: file_path }
              end

              begin
                virtual_file = filesystem.find_file(file_path)
                unless virtual_file
                  next { success: false, error: 'File not found in virtual filesystem', file_path: file_path }
                end

                original = virtual_file.content || ''
                patched, errors = Preset::Patch.apply(original, diff_text)

                if errors.any?
                  {
                    success: false,
                    error: errors.join('; '),
                    file_path: file_path,
                    failed_hunks: errors.length
                  }
                else
                  {
                    success: true,
                    file_path: file_path,
                    patched_content: patched,
                    original_size: original.bytesize,
                    patched_size: patched.bytesize
                  }
                end
              rescue StandardError => e
                { success: false, error: e.message, file_path: file_path }
              end
            end
          end
        end

        # Pure-Ruby unified diff applicator.
        module Patch
          # Applies a unified diff to a string.
          #
          # @param original [String] the original file content
          # @param diff_text [String] a unified diff (--- / +++ / @@ headers + +/-/space lines)
          # @return [Array(String, Array<String>)] [patched_content, errors]
          #   errors is empty on success; contains one message per failed hunk otherwise.
          def self.apply(original, diff_text)
            hunks  = parse_hunks(diff_text)
            lines  = original.lines
            errors = []
            offset = 0 # cumulative line shift from previous hunks

            hunks.each do |hunk|
              start  = hunk[:start_a] - 1 + offset  # 0-based
              dels   = hunk[:lines].select { |l| l[:type] == :del }.map { |l| l[:content] }
              ins    = hunk[:lines].select { |l| l[:type] == :ins }.map { |l| l[:content] }

              # Verify the context/deleted lines match what's actually in the file
              expected_removals = hunk[:lines].select { |l| l[:type] == :del || l[:type] == :ctx }
                                              .map { |l| l[:content] }
              actual_slice = lines[start, expected_removals.length] || []

              # Fuzzy match: strip trailing newline differences for comparison
              mismatch = expected_removals.zip(actual_slice).any? do |exp, act|
                exp.chomp != act&.chomp
              end

              if mismatch
                errors << "Hunk @@ -#{hunk[:start_a]} failed to apply (context mismatch at line #{start + 1})"
                next
              end

              ctx_and_del_count = hunk[:lines].count { |l| l[:type] == :del || l[:type] == :ctx }
              ctx_lines         = hunk[:lines].select { |l| l[:type] == :ctx }.map { |l| l[:content] }
              replacement       = ctx_lines.zip(ins + [nil]).flat_map do |ctx, _|
                # Interleave: keep context lines, insert insertions in sequence
                nil
              end

              # Simpler, correct approach: rebuild from hunk lines
              replacement = hunk[:lines].filter_map do |l|
                case l[:type]
                when :ctx then l[:content]
                when :ins then l[:content]
                when :del then nil
                end
              end

              lines[start, ctx_and_del_count] = replacement
              offset += ins.length - dels.length
            end

            [lines.join, errors]
          end

          # @api private
          # @return [Array<Hash>] each hunk: { start_a:, count_a:, start_b:, count_b:, lines: [{type:, content:}] }
          def self.parse_hunks(diff_text)
            hunks       = []
            current     = nil

            diff_text.each_line do |line|
              # Skip file header lines
              next if line.start_with?('--- ', '+++ ')

              if (m = line.match(/^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/))
                hunks << current if current
                current = {
                  start_a: m[1].to_i,
                  count_a: (m[2] || '1').to_i,
                  start_b: m[3].to_i,
                  count_b: (m[4] || '1').to_i,
                  lines: []
                }
                next
              end

              next unless current

              if line.start_with?('+')
                current[:lines] << { type: :ins, content: line[1..] }
              elsif line.start_with?('-')
                current[:lines] << { type: :del, content: line[1..] }
              elsif line.start_with?(' ') || line == "\n"
                current[:lines] << { type: :ctx, content: line.start_with?(' ') ? line[1..] : line }
              end
            end

            hunks << current if current
            hunks
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
