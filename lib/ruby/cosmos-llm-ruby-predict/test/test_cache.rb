# frozen_string_literal: true

require 'tmpdir'

require_relative 'test_helper'

class TestCache < Minitest::Test
  Cache = Cosmos::Llm::Predict::Cache

  def test_key_is_stable_across_hash_ordering
    a = Cache.key_for(model: 'x', messages: [{ role: 'user' }])
    b = Cache.key_for(messages: [{ role: 'user' }], model: 'x')

    assert_equal a, b
  end

  def test_key_treats_symbol_and_string_keys_alike
    assert_equal Cache.key_for(model: 'x'), Cache.key_for('model' => 'x')
  end

  def test_key_changes_with_content
    refute_equal Cache.key_for(model: 'x'), Cache.key_for(model: 'y')
  end

  def test_fetch_computes_once
    cache = Cache.new
    calls = 0
    block = lambda do
      calls += 1
      'value'
    end

    assert_equal 'value', cache.fetch(model: 'x', &block)
    assert_equal 'value', cache.fetch(model: 'x', &block)
    assert_equal 1, calls
  end

  def test_fetch_requires_a_block
    assert_raises(ArgumentError) { Cache.new.fetch(model: 'x') }
  end

  def test_different_requests_do_not_collide
    cache = Cache.new

    cache.fetch(model: 'x') { 'first' }

    assert_equal 'second', cache.fetch(model: 'y') { 'second' }
  end

  def test_memory_cache_evicts_oldest_entries
    cache = Cache.new(max_memory_entries: 2)
    cache.write('a', 1)
    cache.write('b', 2)
    cache.write('c', 3)

    assert_equal 2, cache.size
    assert_nil cache.read('a')
    assert_equal 3, cache.read('c')[:value]
  end

  def test_reading_an_entry_keeps_it_from_eviction
    cache = Cache.new(max_memory_entries: 2)
    cache.write('a', 1)
    cache.write('b', 2)
    cache.read('a')
    cache.write('c', 3)

    assert_equal 1, cache.read('a')[:value]
    assert_nil cache.read('b')
  end

  def test_disk_cache_survives_a_new_instance
    Dir.mktmpdir do |dir|
      Cache.new(directory: dir).write('key', { 'completion' => 'text' })

      fresh = Cache.new(directory: dir)

      assert_equal({ 'completion' => 'text' }, fresh.read('key')[:value])
    end
  end

  def test_disk_cache_misses_return_nil
    Dir.mktmpdir do |dir|
      assert_nil Cache.new(directory: dir).read('absent')
    end
  end

  def test_clear_empties_both_levels
    Dir.mktmpdir do |dir|
      cache = Cache.new(directory: dir)
      cache.write('key', 'value')
      cache.clear

      assert_equal 0, cache.size
      assert_nil Cache.new(directory: dir).read('key')
    end
  end

  def test_corrupt_disk_entries_are_treated_as_misses
    Dir.mktmpdir do |dir|
      cache = Cache.new(directory: dir, memory: false)
      cache.write('key', 'value')
      path = Dir.glob(File.join(dir, '**', '*.json')).first
      File.write(path, 'not json')

      assert_nil cache.read('key')
    end
  end

  def test_predictor_reuses_cached_completions
    Dir.mktmpdir do |dir|
      client = FakeClient.new("[[ ## answer ## ]]\nyes\n")
      cache = Cache.new(directory: dir)
      predictor = Cosmos::Llm::Predict::Predict.new('question -> answer', client: client, cache: cache)

      first = predictor.call(question: 'Why?')
      second = predictor.call(question: 'Why?')

      assert_equal 1, client.call_count
      assert_equal 'yes', second.answer
      refute first.cached?
      assert second.cached?
    end
  end

  def test_cache_key_separates_different_inputs
    client = FakeClient.new("[[ ## answer ## ]]\nfirst\n", "[[ ## answer ## ]]\nsecond\n")
    predictor = Cosmos::Llm::Predict::Predict.new('question -> answer', client: client, cache: Cache.new)

    assert_equal 'first', predictor.call(question: 'a').answer
    assert_equal 'second', predictor.call(question: 'b').answer
    assert_equal 2, client.call_count
  end

  def test_cache_key_separates_different_instructions
    client = FakeClient.new("[[ ## answer ## ]]\nfirst\n", "[[ ## answer ## ]]\nsecond\n")
    cache = Cache.new
    base = Cosmos::Llm::Predict::Predict.new('question -> answer', client: client, cache: cache)
    variant = base.with_instructions('Be terse.')

    assert_equal 'first', base.call(question: 'a').answer
    assert_equal 'second', variant.call(question: 'a').answer
  end
end
