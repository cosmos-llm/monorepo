# frozen_string_literal: true

require "test_helper"
require "tmpdir"
require "cosmos_llm_markdowner/error"
require "cosmos_llm_markdowner/sandbox"

class TestSandbox < Minitest::Test
  def setup
    @tmpdir = Dir.mktmpdir
    @file   = File.join(@tmpdir, "hello.md")
    File.write(@file, "# Hello")
  end

  def teardown
    FileUtils.rm_rf(@tmpdir)
  end

  def test_read_allowed_file
    sandbox = CosmosLlmMarkdowner::Sandbox.new(allowed_reads: [@file])
    assert_equal "# Hello", sandbox.read(@file)
  end

  def test_read_denied_by_default
    sandbox = CosmosLlmMarkdowner::Sandbox.new
    err = assert_raises(CosmosLlmMarkdowner::FilesystemError) { sandbox.read(@file) }
    assert_match "Read denied", err.message
  end

  def test_write_allowed_explicit_file
    out = File.join(@tmpdir, "out.md")
    sandbox = CosmosLlmMarkdowner::Sandbox.new(allowed_writes: [out])
    sandbox.write(out, "# Out")
    assert_equal "# Out", File.read(out)
  end

  def test_write_allowed_via_dir
    out = File.join(@tmpdir, "docs", "page.md")
    sandbox = CosmosLlmMarkdowner::Sandbox.new(allowed_dirs: [@tmpdir])
    sandbox.write(out, "# Page")
    assert_equal "# Page", File.read(out)
  end

  def test_write_denied_outside_dir
    other = Dir.mktmpdir
    out   = File.join(other, "evil.md")
    sandbox = CosmosLlmMarkdowner::Sandbox.new(allowed_dirs: [@tmpdir])
    assert_raises(CosmosLlmMarkdowner::FilesystemError) { sandbox.write(out, "bad") }
  ensure
    FileUtils.rm_rf(other)
  end

  def test_write_denied_by_default
    out = File.join(@tmpdir, "out.md")
    sandbox = CosmosLlmMarkdowner::Sandbox.new
    assert_raises(CosmosLlmMarkdowner::FilesystemError) { sandbox.write(out, "bad") }
  end

  def test_readable_paths_returns_copy
    sandbox = CosmosLlmMarkdowner::Sandbox.new(allowed_reads: [@file])
    paths = sandbox.readable_paths
    paths.clear
    assert_equal 1, sandbox.readable_paths.size
  end
end
