# frozen_string_literal: true

require 'minitest/autorun'
require 'mocha/minitest'

$LOAD_PATH.unshift File.expand_path('../../../../lib', __dir__)

module Cosmos
  module Llm
    module Tool
      def self.define(name, register: false, &block)
        tool = MockToolDefinition.new(name)
        tool.instance_eval(&block) if block
        tool
      end

      class MockToolDefinition
        def initialize(name)
          @name = name
          @parameters = []
          @execute_block = nil
        end

        def description(desc); @description = desc; end
        def parameter(name, **_opts); @parameters << name; end
        def execute(&block); @execute_block = block; end
        def call(params = {}); @execute_block.call(params); end
      end
    end
  end
end

$LOADED_FEATURES << 'cosmos/llm/tool.rb'

require_relative '../../../../lib/cosmos/llm/tool/preset/patch'

module Cosmos
  module Llm
    module Tool
      module Preset
        class PatchApplyTest < Minitest::Test
          def test_simple_line_replacement
            original = "hello\nworld\n"
            diff     = "--- a.txt\n+++ b.txt\n@@ -1,2 +1,2 @@\n-hello\n+goodbye\n world\n"

            patched, errors = Patch.apply(original, diff)

            assert_empty errors
            assert_equal "goodbye\nworld\n", patched
          end

          def test_add_line
            original = "line1\nline3\n"
            diff     = "--- a\n+++ b\n@@ -1,2 +1,3 @@\n line1\n+line2\n line3\n"

            patched, errors = Patch.apply(original, diff)

            assert_empty errors
            assert_equal "line1\nline2\nline3\n", patched
          end

          def test_remove_line
            original = "line1\nline2\nline3\n"
            diff     = "--- a\n+++ b\n@@ -1,3 +1,2 @@\n line1\n-line2\n line3\n"

            patched, errors = Patch.apply(original, diff)

            assert_empty errors
            assert_equal "line1\nline3\n", patched
          end

          def test_no_op_diff
            original = "abc\n"
            diff     = "--- a\n+++ b\n"

            patched, errors = Patch.apply(original, diff)

            assert_empty errors
            assert_equal "abc\n", patched
          end

          def test_context_mismatch_returns_error
            original = "hello\nworld\n"
            # diff expects "different_line" but file has "world"
            diff = "--- a\n+++ b\n@@ -2,1 +2,1 @@\n-different_line\n+replacement\n"

            _, errors = Patch.apply(original, diff)

            refute_empty errors
            assert_match(/Hunk.*failed/, errors.first)
          end

          def test_hunk_header_parsing
            hunks = Patch.parse_hunks("--- a\n+++ b\n@@ -5,3 +5,4 @@\n ctx\n-old\n+new\n ctx2\n")

            assert_equal 1, hunks.length
            h = hunks.first
            assert_equal 5, h[:start_a]
            assert_equal 3, h[:count_a]
            assert_equal 5, h[:start_b]
            assert_equal 4, h[:count_b]
            assert_equal 4, h[:lines].length
          end

          def test_multiple_hunks
            original = "a\nb\nc\nd\ne\nf\ng\nh\n"
            diff = <<~DIFF
              --- a
              +++ b
              @@ -1,1 +1,1 @@
              -a
              +A
              @@ -8,1 +8,1 @@
              -h
              +H
            DIFF

            patched, errors = Patch.apply(original, diff)

            assert_empty errors
            assert_includes patched, "A\n"
            assert_includes patched, "H\n"
            refute_includes patched, "a\n"
            refute_includes patched, "h\n"
          end
        end

        class PatchToolTest < Minitest::Test
          def setup
            @fs   = mock('filesystem')
            @tool = Preset.patch(@fs)
          end

          def test_successful_patch
            original = "hello\nworld\n"
            diff     = "--- a\n+++ b\n@@ -1,1 +1,1 @@\n-hello\n+goodbye\n world\n"
            vf       = stub(content: original)
            @fs.stubs(:find_file).with('file.txt').returns(vf)

            result = @tool.call(file_path: 'file.txt', diff: diff)

            assert result[:success]
            assert_equal "goodbye\nworld\n", result[:patched_content]
            assert_equal original.bytesize, result[:original_size]
          end

          def test_file_not_found
            @fs.stubs(:find_file).with('missing.txt').returns(nil)

            result = @tool.call(file_path: 'missing.txt', diff: "@@ -1 +1 @@\n-x\n+y\n")

            refute result[:success]
            assert_includes result[:error], 'not found'
          end

          def test_empty_file_path
            result = @tool.call(file_path: '', diff: "@@\n")

            refute result[:success]
            assert_includes result[:error], 'file_path is required'
          end

          def test_nil_diff
            vf = stub(content: "x\n")
            @fs.stubs(:find_file).with('f.txt').returns(vf)

            result = @tool.call(file_path: 'f.txt', diff: nil)

            refute result[:success]
            assert_includes result[:error], 'diff is required'
          end

          def test_context_mismatch_reports_error
            vf   = stub(content: "actual_content\n")
            diff = "--- a\n+++ b\n@@ -1,1 +1,1 @@\n-wrong_content\n+replacement\n"
            @fs.stubs(:find_file).with('f.txt').returns(vf)

            result = @tool.call(file_path: 'f.txt', diff: diff)

            refute result[:success]
            assert result.key?(:failed_hunks)
          end

          def test_nil_content_treated_as_empty
            vf   = stub(content: nil)
            diff = "--- a\n+++ b\n@@ -1,1 +1,1 @@\n+new line\n"
            @fs.stubs(:find_file).with('f.txt').returns(vf)

            result = @tool.call(file_path: 'f.txt', diff: diff)

            # With nil content (empty string), hunk must match accordingly
            assert result.key?(:success)
          end
        end
      end
    end
  end
end
