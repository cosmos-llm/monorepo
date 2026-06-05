# frozen_string_literal: true

require 'minitest/autorun'
require 'mocha/minitest'

$LOAD_PATH.unshift File.expand_path('../../../../lib', __dir__)

# Stub cosmos-llm-tool before loading preset
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

require_relative '../../../../lib/cosmos/llm/tool/preset/diff'

module Cosmos
  module Llm
    module Tool
      module Preset
        class DiffUtilTest < Minitest::Test
          def test_identical_files_produce_empty_diff
            result = Diff.unified_diff("a\nb\nc\n", "a\nb\nc\n", 'a.txt', 'b.txt')
            assert_empty result
          end

          def test_single_line_change
            result = Diff.unified_diff("hello\n", "goodbye\n", 'a.txt', 'b.txt')
            assert_includes result, '--- a.txt'
            assert_includes result, '+++ b.txt'
            assert_includes result, '-hello'
            assert_includes result, '+goodbye'
          end

          def test_added_line
            result = Diff.unified_diff("line1\n", "line1\nline2\n", 'a.txt', 'b.txt')
            assert_includes result, '+line2'
            refute_includes result, '-line1'
          end

          def test_removed_line
            result = Diff.unified_diff("line1\nline2\n", "line1\n", 'a.txt', 'b.txt')
            assert_includes result, '-line2'
          end

          def test_empty_to_content
            result = Diff.unified_diff('', "new content\n", 'a.txt', 'b.txt')
            assert_includes result, '+new content'
          end

          def test_content_to_empty
            result = Diff.unified_diff("old content\n", '', 'a.txt', 'b.txt')
            assert_includes result, '-old content'
          end

          def test_hunk_header_present
            result = Diff.unified_diff("a\n", "b\n", 'a.txt', 'b.txt')
            assert_match(/^@@/, result)
          end
        end

        class DiffToolTest < Minitest::Test
          def setup
            @fs = mock('filesystem')
            @tool = Preset.diff(@fs)
          end

          def test_diff_two_files
            fa = stub(content: "hello\n")
            fb = stub(content: "goodbye\n")
            @fs.stubs(:find_file).with('a.txt').returns(fa)
            @fs.stubs(:find_file).with('b.txt').returns(fb)

            result = @tool.call(path_a: 'a.txt', path_b: 'b.txt')

            assert result[:success]
            assert result[:changed]
            assert_includes result[:diff], '-hello'
            assert_includes result[:diff], '+goodbye'
          end

          def test_diff_file_against_content
            fa = stub(content: "hello\n")
            @fs.stubs(:find_file).with('a.txt').returns(fa)

            result = @tool.call(path_a: 'a.txt', content_b: "goodbye\n")

            assert result[:success]
            assert result[:changed]
            assert_includes result[:diff], '-hello'
            assert_includes result[:diff], '+goodbye'
          end

          def test_identical_files_not_changed
            fa = stub(content: "same\n")
            fb = stub(content: "same\n")
            @fs.stubs(:find_file).with('a.txt').returns(fa)
            @fs.stubs(:find_file).with('b.txt').returns(fb)

            result = @tool.call(path_a: 'a.txt', path_b: 'b.txt')

            assert result[:success]
            refute result[:changed]
            assert_empty result[:diff]
          end

          def test_missing_path_b_and_content_b
            fa = stub(content: "hello\n")
            @fs.stubs(:find_file).with('a.txt').returns(fa)

            result = @tool.call(path_a: 'a.txt')

            refute result[:success]
            assert_includes result[:error], 'Either path_b or content_b'
          end

          def test_both_path_b_and_content_b_is_error
            fa = stub(content: "hello\n")
            @fs.stubs(:find_file).with('a.txt').returns(fa)

            result = @tool.call(path_a: 'a.txt', path_b: 'b.txt', content_b: 'x')

            refute result[:success]
            assert_includes result[:error], 'mutually exclusive'
          end

          def test_path_a_not_found
            @fs.stubs(:find_file).with('missing.txt').returns(nil)

            result = @tool.call(path_a: 'missing.txt', content_b: 'x')

            refute result[:success]
            assert_includes result[:error], 'not found'
          end

          def test_path_b_not_found
            fa = stub(content: "hello\n")
            @fs.stubs(:find_file).with('a.txt').returns(fa)
            @fs.stubs(:find_file).with('missing.txt').returns(nil)

            result = @tool.call(path_a: 'a.txt', path_b: 'missing.txt')

            refute result[:success]
            assert_includes result[:error], 'not found'
          end

          def test_nil_content_treated_as_empty
            fa = stub(content: nil)
            fb = stub(content: "new\n")
            @fs.stubs(:find_file).with('a.txt').returns(fa)
            @fs.stubs(:find_file).with('b.txt').returns(fb)

            result = @tool.call(path_a: 'a.txt', path_b: 'b.txt')

            assert result[:success]
            assert result[:changed]
          end
        end
      end
    end
  end
end
