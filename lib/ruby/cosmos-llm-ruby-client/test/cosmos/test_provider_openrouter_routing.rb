# frozen_string_literal: true

require 'minitest/autorun'
require 'webmock/minitest'
require 'cosmos/llm/providers/openrouter'

# Covers OpenRouter provider routing: the Routing builder, how a preference
# reaches the wire, and how the serving provider is read back off a response.
class TestProviderOpenRouterRouting < Minitest::Test
  Routing = Cosmos::Llm::Providers::OpenRouter::Routing

  COMPLETION_URL = 'https://openrouter.ai/api/v1/chat/completions'

  def setup
    WebMock.disable_net_connect!
    @provider = Cosmos::Llm::Providers::OpenRouter.new(api_key: 'test_api_key')
  end

  def teardown
    WebMock.reset!
  end

  # ── The builder ─────────────────────────────────────────────────────────────

  def test_empty_routing_produces_no_provider_block
    # An untouched preference must not constrain routing at all.
    assert_empty Routing.new.to_h
    assert_empty Routing.new.to_params
    assert_predicate Routing.new, :empty?
  end

  def test_order_only_and_ignore_serialize_as_arrays
    routing = Routing.new.order('azure', 'openai').only('azure', 'openai', 'together')
                     .ignore('deepinfra')

    assert_equal %w[azure openai], routing.to_h['order']
    assert_equal %w[azure openai together], routing.to_h['only']
    assert_equal %w[deepinfra], routing.to_h['ignore']
  end

  def test_order_preserves_the_caller_sequence
    # The whole point of order is the sequence.
    assert_equal %w[c a b], Routing.new.order('c', 'a', 'b').to_h['order']
  end

  def test_list_setters_accept_symbols_and_arrays
    assert_equal %w[azure openai], Routing.new.order(:azure, :openai).to_h['order']
    assert_equal %w[azure openai], Routing.new.order(%w[azure openai]).to_h['order']
  end

  def test_list_setters_replace_rather_than_append
    assert_equal %w[b], Routing.new.only('a').only('b').to_h['only']
  end

  def test_pinned_to_sets_only_and_forbids_fallbacks
    routing = Routing.pinned_to('together')

    assert_equal %w[together], routing.to_h['only']
    assert_equal false, routing.to_h['allow_fallbacks']
  end

  def test_sort_accepts_every_documented_attribute
    Routing::SORTS.each do |attribute|
      assert_equal attribute, Routing.new.sort(attribute).to_h['sort']
    end
  end

  def test_sort_with_a_partition_becomes_an_object
    routing = Routing.new.sort(:price, partition: :none)

    assert_equal({ 'by' => 'price', 'partition' => 'none' }, routing.to_h['sort'])
  end

  def test_sort_rejects_an_unknown_attribute
    # OpenRouter answers an unknown key in this block with a 400, so catching
    # it here saves a network round trip and gives a better message.
    error = assert_raises(ArgumentError) { Routing.new.sort(:cheapest) }
    assert_match(/Unknown sort/, error.message)
    assert_match(/price/, error.message)
  end

  def test_sort_rejects_an_unknown_partition
    assert_raises(ArgumentError) { Routing.new.sort(:price, partition: :everything) }
  end

  def test_data_collection_accepts_allow_and_deny
    assert_equal 'deny', Routing.new.data_collection(:deny).to_h['data_collection']
    assert_equal 'allow', Routing.new.data_collection('allow').to_h['data_collection']
  end

  def test_data_collection_rejects_anything_else
    assert_raises(ArgumentError) { Routing.new.data_collection(:maybe) }
  end

  def test_booleans_are_written_when_set_and_absent_when_not
    routing = Routing.new.allow_fallbacks(false).require_parameters(true)

    assert_equal false, routing.to_h['allow_fallbacks']
    assert_equal true, routing.to_h['require_parameters']

    # false and unset are different instructions, so unset must be absent.
    refute Routing.new.to_h.key?('allow_fallbacks')
    refute Routing.new.to_h.key?('require_parameters')
  end

  def test_booleans_reject_non_boolean_values
    assert_raises(ArgumentError) { Routing.new.allow_fallbacks('no') }
    assert_raises(ArgumentError) { Routing.new.require_parameters(nil) }
  end

  def test_quantizations_validate_against_the_known_levels
    assert_equal %w[fp16 bf16], Routing.new.quantizations('fp16', 'bf16').to_h['quantizations']
    assert_raises(ArgumentError) { Routing.new.quantizations('fp99') }
  end

  def test_max_price_writes_only_the_ceilings_that_are_set
    ceilings = Routing.new.max_price(prompt: 3.0, completion: 15.0).to_h['max_price']

    assert_equal({ 'prompt' => 3.0, 'completion' => 15.0 }, ceilings)
  end

  def test_max_price_rejects_unknown_keys_and_bad_values
    assert_raises(ArgumentError) { Routing.new.max_price(bogus: 1.0) }
    assert_raises(ArgumentError) { Routing.new.max_price(prompt: -1) }
    assert_raises(ArgumentError) { Routing.new.max_price(prompt: 'cheap') }
  end

  def test_zdr_and_distillable_text_are_written
    routing = Routing.new.zdr(true).enforce_distillable_text(false)

    assert_equal true, routing.to_h['zdr']
    assert_equal false, routing.to_h['enforce_distillable_text']
  end

  def test_preferred_throughput_and_latency_are_written
    routing = Routing.new.preferred_min_throughput(40).preferred_max_latency(2.5)

    assert_in_delta 40, routing.to_h['preferred_min_throughput']
    assert_in_delta 2.5, routing.to_h['preferred_max_latency']
  end

  def test_preferences_reject_negative_numbers
    assert_raises(ArgumentError) { Routing.new.preferred_min_throughput(-1) }
    assert_raises(ArgumentError) { Routing.new.preferred_max_latency('slow') }
  end

  def test_from_h_round_trips_a_routing
    routing = Routing.new.order('azure').ignore('deepinfra').allow_fallbacks(false)
                     .require_parameters(true).data_collection(:deny)
                     .quantizations('fp8').max_price(prompt: 1.25).zdr(true)
                     .preferred_min_throughput(30).sort(:latency, partition: :model)

    assert_equal routing, Routing.from_h(routing.to_h)
  end

  def test_from_h_accepts_a_plain_string_sort
    assert_equal 'price', Routing.from_h('sort' => 'price').to_h['sort']
  end

  # ── Reaching the wire ───────────────────────────────────────────────────────

  def stub_completion(body: nil, request_body: nil)
    stub = stub_request(:post, COMPLETION_URL)
    stub = stub.with(body: hash_including(request_body)) if request_body
    stub.to_return(
      status: 200,
      body: (body || {
        choices: [{ message: { role: 'assistant', content: 'ok' } }]
      }).to_json,
      headers: { 'Content-Type' => 'application/json' }
    )
  end

  def test_completion_sends_a_per_request_routing_block
    stub = stub_completion(
      request_body: { 'provider' => { 'only' => ['together'], 'allow_fallbacks' => false } }
    )

    @provider.completion(
      model: 'openai/gpt-4o',
      messages: [{ role: 'user', content: 'Hello' }],
      **Routing.pinned_to('together').to_params
    )

    assert_requested stub
  end

  def test_client_wide_routing_is_applied_to_every_request
    provider = Cosmos::Llm::Providers::OpenRouter.new(
      api_key: 'test_api_key',
      routing: Routing.new.sort(:price)
    )
    stub = stub_completion(request_body: { 'provider' => { 'sort' => 'price' } })

    provider.completion(model: 'openai/gpt-4o', messages: [{ role: 'user', content: 'Hi' }])

    assert_requested stub
  end

  def test_a_per_request_block_replaces_the_client_wide_default
    # Merging instead of replacing would let the client-wide `only` leak into a
    # request that asked to route purely by throughput.
    provider = Cosmos::Llm::Providers::OpenRouter.new(
      api_key: 'test_api_key',
      routing: Routing.new.only('azure')
    )
    stub = stub_request(:post, COMPLETION_URL)
           .with { |req| JSON.parse(req.body)['provider'] == { 'sort' => 'throughput' } }
           .to_return(status: 200,
                      body: { choices: [{ message: { role: 'assistant', content: 'ok' } }] }.to_json,
                      headers: { 'Content-Type' => 'application/json' })

    provider.completion(
      model: 'openai/gpt-4o',
      messages: [{ role: 'user', content: 'Hi' }],
      **Routing.new.sort(:throughput).to_params
    )

    assert_requested stub
  end

  def test_no_provider_block_is_sent_when_no_routing_is_configured
    stub = stub_request(:post, COMPLETION_URL)
           .with { |req| !JSON.parse(req.body).key?('provider') }
           .to_return(status: 200,
                      body: { choices: [{ message: { role: 'assistant', content: 'ok' } }] }.to_json,
                      headers: { 'Content-Type' => 'application/json' })

    @provider.completion(model: 'openai/gpt-4o', messages: [{ role: 'user', content: 'Hi' }])

    assert_requested stub
  end

  def test_provider_accepts_routing_as_a_hash
    provider = Cosmos::Llm::Providers::OpenRouter.new(
      api_key: 'test_api_key',
      routing: { 'sort' => 'latency' }
    )

    assert_equal 'latency', provider.routing.to_h['sort']
  end

  def test_provider_rejects_routing_of_the_wrong_type
    assert_raises(ArgumentError) do
      Cosmos::Llm::Providers::OpenRouter.new(api_key: 'k', routing: 'fastest')
    end
  end

  # ── Route reporting ─────────────────────────────────────────────────────────

  def test_route_reporting_sends_the_metadata_header
    provider = Cosmos::Llm::Providers::OpenRouter.new(api_key: 'k', report_route: true)
    stub = stub_request(:post, COMPLETION_URL)
           .with(headers: { 'X-OpenRouter-Metadata' => 'enabled' })
           .to_return(status: 200,
                      body: { choices: [{ message: { role: 'assistant', content: 'ok' } }] }.to_json,
                      headers: { 'Content-Type' => 'application/json' })

    provider.completion(model: 'openai/gpt-4o', messages: [{ role: 'user', content: 'Hi' }])

    assert_requested stub
  end

  def test_the_header_is_absent_by_default
    stub = stub_request(:post, COMPLETION_URL)
           .with { |req| !req.headers.key?('X-Openrouter-Metadata') }
           .to_return(status: 200,
                      body: { choices: [{ message: { role: 'assistant', content: 'ok' } }] }.to_json,
                      headers: { 'Content-Type' => 'application/json' })

    @provider.completion(model: 'openai/gpt-4o', messages: [{ role: 'user', content: 'Hi' }])

    assert_requested stub
  end

  def test_response_reports_the_selected_upstream
    stub_completion(body: {
                      choices: [{ message: { role: 'assistant', content: 'ok' } }],
                      openrouter_metadata: {
                        attempt: 2,
                        endpoints: {
                          available: [
                            { provider: 'Together', model: 'x/y', selected: false },
                            { provider: 'DeepInfra', model: 'x/y', selected: true }
                          ]
                        }
                      }
                    })

    response = @provider.completion(model: 'x/y', messages: [{ role: 'user', content: 'Hi' }])

    assert_equal 'DeepInfra', response.served_by
    assert_equal 'x/y', response.served_model
    # Two attempts means the first choice failed -- the signal that a routing
    # preference was not honoured.
    assert_equal 2, response.routing_attempts
  end

  def test_response_falls_back_to_the_last_attempt
    stub_completion(body: {
                      choices: [{ message: { role: 'assistant', content: 'ok' } }],
                      openrouter_metadata: {
                        attempt: 1,
                        attempts: [{ provider: 'OpenAI', model: 'openai/gpt-4o', status: 200 }]
                      }
                    })

    response = @provider.completion(model: 'openai/gpt-4o',
                                    messages: [{ role: 'user', content: 'Hi' }])

    assert_equal 'OpenAI', response.served_by
  end

  def test_response_reports_nothing_without_metadata
    stub_completion

    response = @provider.completion(model: 'openai/gpt-4o',
                                    messages: [{ role: 'user', content: 'Hi' }])

    # Absent metadata is "not stated", not an error.
    assert_nil response.served_by
    assert_nil response.served_model
    assert_nil response.routing_attempts
  end
end
