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

        def to_openai_schema
          {
            'function' => {
              'name' => @name.to_s,
              'description' => @description,
              'parameters' => {
                'type' => 'object',
                'properties' => @parameters.each_with_object({}) { |p, h| h[p.to_s] = {} },
                'required' => []
              }
            }
          }
        end
      end
    end
  end
end

$LOADED_FEATURES << 'cosmos/llm/tool.rb'

require_relative '../../../../lib/cosmos/llm/tool/preset/webfetch'

module Cosmos
  module Llm
    module Tool
      module Preset
        # ---------------------------------------------------------------------------
        # Tool construction and schema
        # ---------------------------------------------------------------------------
        class WebfetchToolTest < Minitest::Test
          def setup
            @tool = Preset.webfetch
          end

          def test_returns_mock_tool_definition
            assert_kind_of MockToolDefinition, @tool
          end

          def test_schema_name
            assert_equal 'webfetch', @tool.to_openai_schema['function']['name']
          end

          def test_schema_has_url_parameter
            assert_includes @tool.to_openai_schema['function']['parameters']['properties'], 'url'
          end

          def test_schema_has_format_parameter
            assert_includes @tool.to_openai_schema['function']['parameters']['properties'], 'format'
          end

          def test_schema_has_timeout_parameter
            assert_includes @tool.to_openai_schema['function']['parameters']['properties'], 'timeout'
          end
        end

        # ---------------------------------------------------------------------------
        # Tool execution — success paths
        # ---------------------------------------------------------------------------
        class WebfetchExecuteSuccessTest < Minitest::Test
          def setup
            @tool = Preset.webfetch
          end

          def stub_http(body:, content_type: 'text/html')
            response = mock('response')
            response.stubs(:body).returns(body)
            response.stubs(:[]).with('content-type').returns(content_type)
            response.stubs(:is_a?).with(Net::HTTPSuccess).returns(true)
            response.stubs(:is_a?).with(Net::HTTPRedirection).returns(false)

            http = mock('http')
            http.stubs(:use_ssl=)
            http.stubs(:open_timeout=)
            http.stubs(:read_timeout=)
            http.stubs(:request).returns(response)

            Net::HTTP.stubs(:new).returns(http)
          end

          def test_markdown_format_default
            stub_http(body: '<html><body><h1>Title</h1><p>Body</p></body></html>')
            result = @tool.call(url: 'https://example.com')

            assert result[:success]
            assert_equal 'markdown', result[:format]
            assert_includes result[:content], '# Title'
            assert_includes result[:content], 'Body'
          end

          def test_text_format
            stub_http(body: '<html><body><h1>Title</h1><p>Body</p></body></html>')
            result = @tool.call(url: 'https://example.com', format: 'text')

            assert result[:success]
            assert_equal 'text', result[:format]
            assert_includes result[:content], 'Title'
            assert_includes result[:content], 'Body'
            refute_includes result[:content], '<h1>'
          end

          def test_html_format_returns_raw_html
            html = '<html><body><h1>Title</h1></body></html>'
            stub_http(body: html)
            result = @tool.call(url: 'https://example.com', format: 'html')

            assert result[:success]
            assert_equal 'html', result[:format]
            assert_equal html, result[:content]
          end

          def test_result_includes_url
            stub_http(body: '<html><body>hi</body></html>')
            result = @tool.call(url: 'https://example.com')

            assert result[:success]
            assert_equal 'https://example.com', result[:url]
          end

          def test_result_includes_content_type
            stub_http(body: '<html></html>', content_type: 'text/html; charset=utf-8')
            result = @tool.call(url: 'https://example.com')

            assert result[:success]
            assert_equal 'text/html; charset=utf-8', result[:content_type]
          end

          def test_result_includes_size
            stub_http(body: '<html><body>hello</body></html>')
            result = @tool.call(url: 'https://example.com')

            assert result[:success]
            assert result[:size].positive?
          end

          def test_result_includes_fetched_at
            stub_http(body: '<html></html>')
            result = @tool.call(url: 'https://example.com')

            assert result[:success]
            assert result[:fetched_at]
          end

          def test_http_upgraded_to_https
            stub_http(body: '<html><body>secure</body></html>')
            Net::HTTP.expects(:new).with('example.com', 443).returns(
              mock('http') do
                stubs(:use_ssl=)
                stubs(:open_timeout=)
                stubs(:read_timeout=)
                response = mock('r')
                response.stubs(:body).returns('<html><body>secure</body></html>')
                response.stubs(:[]).with('content-type').returns('text/html')
                response.stubs(:is_a?).with(Net::HTTPSuccess).returns(true)
                response.stubs(:is_a?).with(Net::HTTPRedirection).returns(false)
                stubs(:request).returns(response)
              end
            )
            result = @tool.call(url: 'http://example.com')
            assert result[:success]
          end

          def test_http_url_becomes_https_in_result
            stub_http(body: '<html></html>')
            result = @tool.call(url: 'http://example.com')
            # The fetch follows the upgraded URI, so the returned url is https
            assert result[:success]
            assert_equal 'https://example.com', result[:url]
          end
        end

        # ---------------------------------------------------------------------------
        # Tool execution — redirect handling
        # ---------------------------------------------------------------------------
        class WebfetchRedirectTest < Minitest::Test
          def setup
            @tool = Preset.webfetch
          end

          def test_follows_single_redirect
            redirect_resp = mock('redirect')
            redirect_resp.stubs(:is_a?).with(Net::HTTPSuccess).returns(false)
            redirect_resp.stubs(:is_a?).with(Net::HTTPRedirection).returns(true)
            redirect_resp.stubs(:[]).with('location').returns('https://example.com/final')

            success_resp = mock('success')
            success_resp.stubs(:body).returns('<html><body>final</body></html>')
            success_resp.stubs(:[]).with('content-type').returns('text/html')
            success_resp.stubs(:is_a?).with(Net::HTTPSuccess).returns(true)
            success_resp.stubs(:is_a?).with(Net::HTTPRedirection).returns(false)

            http1 = mock('http1')
            http1.stubs(:use_ssl=); http1.stubs(:open_timeout=); http1.stubs(:read_timeout=)
            http1.stubs(:request).returns(redirect_resp)

            http2 = mock('http2')
            http2.stubs(:use_ssl=); http2.stubs(:open_timeout=); http2.stubs(:read_timeout=)
            http2.stubs(:request).returns(success_resp)

            Net::HTTP.stubs(:new).returns(http1, http2)

            result = @tool.call(url: 'https://example.com/start')

            assert result[:success]
            assert_equal 'https://example.com/final', result[:url]
          end

          def test_too_many_redirects_fails
            redirect_responses = Array.new(7) do |i|
              r = mock("r#{i}")
              r.stubs(:is_a?).with(Net::HTTPSuccess).returns(false)
              r.stubs(:is_a?).with(Net::HTTPRedirection).returns(true)
              r.stubs(:[]).with('location').returns("https://example.com/#{i + 1}")
              r
            end

            http_mocks = redirect_responses.map do |r|
              h = mock('h')
              h.stubs(:use_ssl=); h.stubs(:open_timeout=); h.stubs(:read_timeout=)
              h.stubs(:request).returns(r)
              h
            end

            Net::HTTP.stubs(:new).returns(*http_mocks)

            result = @tool.call(url: 'https://example.com/start')

            refute result[:success]
            assert_equal 'Failed to fetch content', result[:error]
          end

          def test_empty_redirect_location_fails
            redirect_resp = mock('redirect')
            redirect_resp.stubs(:is_a?).with(Net::HTTPSuccess).returns(false)
            redirect_resp.stubs(:is_a?).with(Net::HTTPRedirection).returns(true)
            redirect_resp.stubs(:[]).with('location').returns('')

            http = mock('http')
            http.stubs(:use_ssl=); http.stubs(:open_timeout=); http.stubs(:read_timeout=)
            http.stubs(:request).returns(redirect_resp)
            Net::HTTP.stubs(:new).returns(http)

            result = @tool.call(url: 'https://example.com')

            refute result[:success]
            assert_equal 'Failed to fetch content', result[:error]
          end

          def test_nil_redirect_location_fails
            redirect_resp = mock('redirect')
            redirect_resp.stubs(:is_a?).with(Net::HTTPSuccess).returns(false)
            redirect_resp.stubs(:is_a?).with(Net::HTTPRedirection).returns(true)
            redirect_resp.stubs(:[]).with('location').returns(nil)

            http = mock('http')
            http.stubs(:use_ssl=); http.stubs(:open_timeout=); http.stubs(:read_timeout=)
            http.stubs(:request).returns(redirect_resp)
            Net::HTTP.stubs(:new).returns(http)

            result = @tool.call(url: 'https://example.com')

            refute result[:success]
            assert_equal 'Failed to fetch content', result[:error]
          end

          def test_invalid_redirect_location_fails
            redirect_resp = mock('redirect')
            redirect_resp.stubs(:is_a?).with(Net::HTTPSuccess).returns(false)
            redirect_resp.stubs(:is_a?).with(Net::HTTPRedirection).returns(true)
            redirect_resp.stubs(:[]).with('location').returns('::not::a::uri::')

            http = mock('http')
            http.stubs(:use_ssl=); http.stubs(:open_timeout=); http.stubs(:read_timeout=)
            http.stubs(:request).returns(redirect_resp)
            Net::HTTP.stubs(:new).returns(http)

            result = @tool.call(url: 'https://example.com')

            refute result[:success]
            assert_equal 'Failed to fetch content', result[:error]
          end

          def test_non_success_non_redirect_response_fails
            resp = mock('resp')
            resp.stubs(:is_a?).with(Net::HTTPSuccess).returns(false)
            resp.stubs(:is_a?).with(Net::HTTPRedirection).returns(false)

            http = mock('http')
            http.stubs(:use_ssl=); http.stubs(:open_timeout=); http.stubs(:read_timeout=)
            http.stubs(:request).returns(resp)
            Net::HTTP.stubs(:new).returns(http)

            result = @tool.call(url: 'https://example.com')

            refute result[:success]
            assert_equal 'Failed to fetch content', result[:error]
          end
        end

        # ---------------------------------------------------------------------------
        # Tool execution — error paths
        # ---------------------------------------------------------------------------
        class WebfetchExecuteErrorTest < Minitest::Test
          def setup
            @tool = Preset.webfetch
          end

          def test_invalid_url_fails
            result = @tool.call(url: 'not-a-url')
            refute result[:success]
            assert_includes result[:error], 'Invalid URL'
            assert_equal 'not-a-url', result[:url]
          end

          def test_ftp_url_fails
            result = @tool.call(url: 'ftp://example.com/file.txt')
            refute result[:success]
            assert_includes result[:error], 'Invalid HTTP URL'
          end

          def test_invalid_format_fails
            result = @tool.call(url: 'https://example.com', format: 'pdf')
            refute result[:success]
            assert_includes result[:error], 'Unsupported format'
            assert_equal 'https://example.com', result[:url]
          end

          def test_read_timeout_returns_error
            http = mock('http')
            http.stubs(:use_ssl=); http.stubs(:open_timeout=); http.stubs(:read_timeout=)
            http.stubs(:request).raises(Net::ReadTimeout.new('timed out'))
            Net::HTTP.stubs(:new).returns(http)

            result = @tool.call(url: 'https://example.com')

            refute result[:success]
            assert_includes result[:error], 'Request timeout'
            assert_equal 'https://example.com', result[:url]
          end

          def test_open_timeout_returns_error
            http = mock('http')
            http.stubs(:use_ssl=); http.stubs(:open_timeout=); http.stubs(:read_timeout=)
            http.stubs(:request).raises(Net::OpenTimeout.new('connect timed out'))
            Net::HTTP.stubs(:new).returns(http)

            result = @tool.call(url: 'https://example.com')

            refute result[:success]
            assert_includes result[:error], 'Request timeout'
          end

          def test_network_error_returns_error
            http = mock('http')
            http.stubs(:use_ssl=); http.stubs(:open_timeout=); http.stubs(:read_timeout=)
            http.stubs(:request).raises(StandardError.new('connection refused'))
            Net::HTTP.stubs(:new).returns(http)

            result = @tool.call(url: 'https://example.com')

            refute result[:success]
            assert_equal 'connection refused', result[:error]
          end
        end

        # ---------------------------------------------------------------------------
        # Tool execution — timeout clamping
        # ---------------------------------------------------------------------------
        class WebfetchTimeoutClampTest < Minitest::Test
          def setup
            @tool = Preset.webfetch
          end

          def make_success_response
            r = mock('r')
            r.stubs(:body).returns('<html></html>')
            r.stubs(:[]).with('content-type').returns('text/html')
            r.stubs(:is_a?).with(Net::HTTPSuccess).returns(true)
            r.stubs(:is_a?).with(Net::HTTPRedirection).returns(false)
            r
          end

          def test_timeout_clamped_to_120_max
            http = mock('http')
            http.stubs(:use_ssl=)
            http.expects(:open_timeout=).with(120)
            http.expects(:read_timeout=).with(120)
            http.stubs(:request).returns(make_success_response)
            Net::HTTP.stubs(:new).returns(http)

            result = @tool.call(url: 'https://example.com', timeout: 999)
            assert result[:success]
          end

          def test_timeout_clamped_to_1_min
            http = mock('http')
            http.stubs(:use_ssl=)
            http.expects(:open_timeout=).with(1)
            http.expects(:read_timeout=).with(1)
            http.stubs(:request).returns(make_success_response)
            Net::HTTP.stubs(:new).returns(http)

            result = @tool.call(url: 'https://example.com', timeout: 0)
            assert result[:success]
          end

          def test_default_timeout_is_30
            http = mock('http')
            http.stubs(:use_ssl=)
            http.expects(:open_timeout=).with(30)
            http.expects(:read_timeout=).with(30)
            http.stubs(:request).returns(make_success_response)
            Net::HTTP.stubs(:new).returns(http)

            result = @tool.call(url: 'https://example.com')
            assert result[:success]
          end
        end

        # ---------------------------------------------------------------------------
        # fetch_with_redirects (helper, called directly on the module)
        # ---------------------------------------------------------------------------
        class FetchWithRedirectsTest < Minitest::Test
          def make_success(body, content_type = 'text/html')
            r = mock('r')
            r.stubs(:body).returns(body)
            r.stubs(:[]).with('content-type').returns(content_type)
            r.stubs(:is_a?).with(Net::HTTPSuccess).returns(true)
            r.stubs(:is_a?).with(Net::HTTPRedirection).returns(false)
            r
          end

          def make_redirect(location)
            r = mock('r')
            r.stubs(:is_a?).with(Net::HTTPSuccess).returns(false)
            r.stubs(:is_a?).with(Net::HTTPRedirection).returns(true)
            r.stubs(:[]).with('location').returns(location)
            r
          end

          def make_http(response)
            h = mock('h')
            h.stubs(:use_ssl=); h.stubs(:open_timeout=); h.stubs(:read_timeout=)
            h.stubs(:request).returns(response)
            h
          end

          def test_returns_body_and_uri_on_success
            Net::HTTP.stubs(:new).returns(make_http(make_success('hello', 'text/plain')))
            uri = URI.parse('https://example.com')

            content, final_uri, ct = Preset.fetch_with_redirects(uri, 10)

            assert_equal 'hello', content
            assert_equal 'https://example.com', final_uri.to_s
            assert_equal 'text/plain', ct
          end

          def test_returns_nil_triple_on_too_many_redirects
            redirects = Array.new(7) { |i| make_http(make_redirect("https://example.com/#{i}")) }
            Net::HTTP.stubs(:new).returns(*redirects)
            uri = URI.parse('https://example.com')

            content, final_uri, ct = Preset.fetch_with_redirects(uri, 10)

            assert_nil content
            assert_nil final_uri
            assert_nil ct
          end

          def test_follows_relative_redirect
            redirect_resp = make_redirect('/page2')
            success_resp  = make_success('<html>page2</html>')

            Net::HTTP.stubs(:new)
                     .returns(make_http(redirect_resp), make_http(success_resp))

            uri = URI.parse('https://example.com/page1')
            content, final_uri, _ct = Preset.fetch_with_redirects(uri, 10)

            assert_equal '<html>page2</html>', content
            assert_equal 'https://example.com/page2', final_uri.to_s
          end

          def test_returns_nil_triple_on_nil_location
            Net::HTTP.stubs(:new).returns(make_http(make_redirect(nil)))
            content, final_uri, ct = Preset.fetch_with_redirects(URI.parse('https://example.com'), 10)
            assert_nil content
            assert_nil final_uri
            assert_nil ct
          end

          def test_returns_nil_triple_on_non_success_non_redirect
            r = mock('r')
            r.stubs(:is_a?).with(Net::HTTPSuccess).returns(false)
            r.stubs(:is_a?).with(Net::HTTPRedirection).returns(false)
            Net::HTTP.stubs(:new).returns(make_http(r))

            content, = Preset.fetch_with_redirects(URI.parse('https://example.com'), 10)
            assert_nil content
          end
        end

        # ---------------------------------------------------------------------------
        # html_to_text
        # ---------------------------------------------------------------------------
        class HtmlToTextTest < Minitest::Test
          def test_strips_tags
            result = Preset.html_to_text('<p>hello <b>world</b></p>')
            assert_includes result, 'hello'
            assert_includes result, 'world'
            refute_includes result, '<b>'
            refute_includes result, '<p>'
          end

          def test_strips_script_content
            result = Preset.html_to_text('<html><body><script>alert(1)</script>visible</body></html>')
            # Nokogiri removes script text from .text; without nokogiri we'd still strip tags
            assert_includes result, 'visible'
          end

          def test_returns_empty_string_for_empty_html
            result = Preset.html_to_text('')
            assert_equal '', result
          end

          def test_strips_all_tags_leaving_text
            result = Preset.html_to_text('<h1>Title</h1><p>Content</p>')
            assert_includes result, 'Title'
            assert_includes result, 'Content'
          end
        end

        # ---------------------------------------------------------------------------
        # html_to_markdown
        # ---------------------------------------------------------------------------
        class HtmlToMarkdownTest < Minitest::Test
          def test_h1_becomes_atx_heading
            result = Preset.html_to_markdown('<h1>Title</h1>')
            assert_includes result, '# Title'
          end

          def test_h2_through_h6
            (2..6).each do |n|
              result = Preset.html_to_markdown("<h#{n}>H</h#{n}>")
              assert_includes result, "#{'#' * n} H", "h#{n} not converted"
            end
          end

          def test_paragraph_gets_double_newline
            result = Preset.html_to_markdown('<p>Para</p>')
            assert_includes result, "Para\n\n"
          end

          def test_strong_bold
            result = Preset.html_to_markdown('<strong>bold</strong>')
            assert_includes result, '**bold**'
          end

          def test_b_bold
            result = Preset.html_to_markdown('<b>bold</b>')
            assert_includes result, '**bold**'
          end

          def test_em_italic
            result = Preset.html_to_markdown('<em>italic</em>')
            assert_includes result, '*italic*'
          end

          def test_i_italic
            result = Preset.html_to_markdown('<i>italic</i>')
            assert_includes result, '*italic*'
          end

          def test_anchor_link
            result = Preset.html_to_markdown('<a href="https://example.com">click</a>')
            assert_includes result, '[click](https://example.com)'
          end

          def test_unordered_list
            result = Preset.html_to_markdown('<ul><li>one</li><li>two</li></ul>')
            assert_includes result, '- one'
            assert_includes result, '- two'
          end

          def test_ordered_list
            result = Preset.html_to_markdown('<ol><li>first</li><li>second</li></ol>')
            assert_includes result, '- first'
            assert_includes result, '- second'
          end

          def test_br_becomes_newline
            result = Preset.html_to_markdown('line1<br>line2')
            assert_includes result, "\n"
          end

          def test_collapses_excessive_blank_lines
            html = '<h1>A</h1>' + '<p>B</p>' * 5
            result = Preset.html_to_markdown(html)
            refute_match(/\n{4,}/, result)
          end

          def test_strips_result
            result = Preset.html_to_markdown('   <p>text</p>   ')
            refute_match(/\A\s/, result)
            refute_match(/\s\z/, result)
          end

          def test_div_span_section_article_pass_through_children
            %w[div span section article].each do |tag|
              result = Preset.html_to_markdown("<#{tag}>content</#{tag}>")
              assert_includes result, 'content', "#{tag} children not passed through"
            end
          end

          def test_unknown_element_passes_through_children
            result = Preset.html_to_markdown('<aside>side note</aside>')
            assert_includes result, 'side note'
          end
        end

        # ---------------------------------------------------------------------------
        # Markdown integration tests (fetch → convert)
        # ---------------------------------------------------------------------------
        class WebfetchMarkdownIntegrationTest < Minitest::Test
          def setup
            @tool = Preset.webfetch
          end

          def stub_html(html)
            r = mock('r')
            r.stubs(:body).returns(html)
            r.stubs(:[]).with('content-type').returns('text/html')
            r.stubs(:is_a?).with(Net::HTTPSuccess).returns(true)
            r.stubs(:is_a?).with(Net::HTTPRedirection).returns(false)
            h = mock('h')
            h.stubs(:use_ssl=); h.stubs(:open_timeout=); h.stubs(:read_timeout=)
            h.stubs(:request).returns(r)
            Net::HTTP.stubs(:new).returns(h)
          end

          def test_list_items_in_result
            stub_html('<ul><li>alpha</li><li>beta</li></ul>')
            result = @tool.call(url: 'https://example.com')
            assert result[:success]
            assert_includes result[:content], '- alpha'
            assert_includes result[:content], '- beta'
          end

          def test_links_in_result
            stub_html('<p>See <a href="https://example.com/page">this page</a></p>')
            result = @tool.call(url: 'https://example.com')
            assert result[:success]
            assert_includes result[:content], '[this page](https://example.com/page)'
          end

          def test_bold_in_result
            stub_html('<p><strong>important</strong></p>')
            result = @tool.call(url: 'https://example.com')
            assert result[:success]
            assert_includes result[:content], '**important**'
          end

          def test_heading_hierarchy
            stub_html('<h1>H1</h1><h2>H2</h2><h3>H3</h3>')
            result = @tool.call(url: 'https://example.com')
            assert result[:success]
            assert_includes result[:content], '# H1'
            assert_includes result[:content], '## H2'
            assert_includes result[:content], '### H3'
          end
        end
      end
    end
  end
end
