# frozen_string_literal: true

require 'minitest/autorun'
require 'mocha/minitest'

# Add lib to load path
$LOAD_PATH.unshift File.expand_path('../../../lib', __dir__)

# Mock the cosmos-llm-tool dependency before requiring anything
module Cosmos
  module Llm
    module Tool
      def self.define(name, register: false, &block) # rubocop:disable Lint/UnusedMethodArgument
        # Create a mock tool definition that can execute the block
        tool = MockToolDefinition.new(name)
        tool.instance_eval(&block) if block
        tool
      end

      # Mock the Definition class
      class Definition
        def self.new(*args)
          MockToolDefinition.new(*args)
        end
      end

      class MockToolDefinition
        def initialize(name, &block)
          @name = name
          @block = block
          @parameters = []
        end

        def description(desc)
          @description = desc
        end

        def parameter(name, **options)
          @parameters << { name: name, **options }
        end

        def execute(&block)
          @execute_block = block
        end

        def call(params = {})
          # Execute the stored block with the parameters
          @execute_block&.call(params)
        rescue StandardError => e
          { success: false, error: e.message }
        end

        # Delegate helper method calls to the Preset module
        def method_missing(method_name, *args, &block)
          if Cosmos::Llm::Tool::Preset.respond_to?(method_name)
            Cosmos::Llm::Tool::Preset.send(method_name, *args, &block)
          else
            super
          end
        end

        def respond_to_missing?(method_name, include_private = false)
          Cosmos::Llm::Tool::Preset.respond_to?(method_name) || super
        end

        def to_openai_schema
          {
            'function' => {
              'name' => @name.to_s,
              'description' => @description,
              'parameters' => {
                'type' => 'object',
                'properties' => @parameters.each_with_object({}) do |param, hash|
                  hash[param[:name].to_s] = {
                    'type' => param[:type].to_s,
                    'description' => param[:description]
                  }
                  hash[param[:name].to_s]['required'] = param[:required] if param.key?(:required)
                end,
                'required' => @parameters.select { |p| p[:required] }.map { |p| p[:name].to_s }
              }
            }
          }
        end

        def to_anthropic_schema
          # Mock anthropic schema
          { 'name' => @name.to_s, 'description' => @description }
        end
      end
    end
  end
end

# Mock the require to avoid loading the actual gem
$LOADED_FEATURES << 'cosmos/llm/tool.rb'

require_relative '../../../../lib/cosmos/llm/tool/preset'

module Cosmos
  module Llm
    module Tool
      class WebfetchTest < Minitest::Test
        def setup
          @webfetch_tool = Cosmos::Llm::Tool::Preset.webfetch
        end

        def test_webfetch_tool_creation
          assert_kind_of Cosmos::Llm::Tool::MockToolDefinition, @webfetch_tool
        end

        def test_webfetch_tool_schema
          schema = @webfetch_tool.to_openai_schema
          assert_equal 'webfetch', schema['function']['name']
          assert_includes schema['function']['description'], 'Fetch content from URLs'
          assert_includes schema['function']['parameters']['properties'], 'url'
          assert_includes schema['function']['parameters']['properties'], 'format'
          assert_includes schema['function']['parameters']['properties'], 'timeout'
        end

        def test_webfetch_successful_html_fetch
          html_content = '<html><body><h1>Test</h1><p>Content</p></body></html>'
          mock_response = mock('response')
          mock_response.stubs(:body).returns(html_content)
          mock_response.stubs(:[]).with('content-type').returns('text/html')
          mock_response.stubs(:is_a?).with(Net::HTTPSuccess).returns(true)
          mock_response.stubs(:is_a?).with(Net::HTTPRedirection).returns(false)

          mock_http = mock('http')
          mock_http.stubs(:use_ssl=)
          mock_http.stubs(:open_timeout=)
          mock_http.stubs(:read_timeout=)
          mock_http.stubs(:request).returns(mock_response)

          Net::HTTP.stubs(:new).returns(mock_http)

          result = @webfetch_tool.call(url: 'https://example.com')

          assert result[:success]
          assert_equal 'https://example.com', result[:url]
          assert_equal 'markdown', result[:format]
          assert_includes result[:content], '# Test'
          assert_includes result[:content], 'Content'
          assert_equal 'text/html', result[:content_type]
          assert result[:size].positive?
          assert result[:fetched_at]
        end

        def test_webfetch_successful_text_format
          html_content = '<html><body><h1>Test</h1><p>Content</p></body></html>'
          mock_response = mock('response')
          mock_response.stubs(:body).returns(html_content)
          mock_response.stubs(:[]).with('content-type').returns('text/html')
          mock_response.stubs(:is_a?).with(Net::HTTPSuccess).returns(true)
          mock_response.stubs(:is_a?).with(Net::HTTPRedirection).returns(false)

          mock_http = mock('http')
          mock_http.stubs(:use_ssl=)
          mock_http.stubs(:open_timeout=)
          mock_http.stubs(:read_timeout=)
          mock_http.stubs(:request).returns(mock_response)

          Net::HTTP.stubs(:new).returns(mock_http)

          result = @webfetch_tool.call(url: 'https://example.com', format: 'text')

          assert result[:success]
          assert_equal 'text', result[:format]
          assert_includes result[:content], 'Test'
          assert_includes result[:content], 'Content'
          refute_includes result[:content], '# ' # No markdown headers
        end

        def test_webfetch_successful_html_format
          html_content = '<html><body><h1>Test</h1></body></html>'
          mock_response = mock('response')
          mock_response.stubs(:body).returns(html_content)
          mock_response.stubs(:[]).with('content-type').returns('text/html')
          mock_response.stubs(:is_a?).with(Net::HTTPSuccess).returns(true)
          mock_response.stubs(:is_a?).with(Net::HTTPRedirection).returns(false)

          mock_http = mock('http')
          mock_http.stubs(:use_ssl=)
          mock_http.stubs(:open_timeout=)
          mock_http.stubs(:read_timeout=)
          mock_http.stubs(:request).returns(mock_response)

          Net::HTTP.stubs(:new).returns(mock_http)

          result = @webfetch_tool.call(url: 'https://example.com', format: 'html')

          assert result[:success]
          assert_equal 'html', result[:format]
          assert_equal html_content, result[:content]
        end

        def test_webfetch_http_upgrade_to_https
          html_content = '<html><body>Test</body></html>'
          mock_response = mock('response')
          mock_response.stubs(:body).returns(html_content)
          mock_response.stubs(:[]).with('content-type').returns('text/html')
          mock_response.stubs(:is_a?).with(Net::HTTPSuccess).returns(true)
          mock_response.stubs(:is_a?).with(Net::HTTPRedirection).returns(false)

          mock_http = mock('http')
          mock_http.stubs(:use_ssl=).with(true) # Should set SSL for HTTPS
          mock_http.stubs(:open_timeout=)
          mock_http.stubs(:read_timeout=)
          mock_http.stubs(:request).returns(mock_response)

          Net::HTTP.stubs(:new).with('example.com', 443).returns(mock_http)

          result = @webfetch_tool.call(url: 'http://example.com')

          assert result[:success]
          assert_equal 'https://example.com', result[:url] # Should be upgraded
        end

        def test_webfetch_with_redirect
          html_content = '<html><body>Final content</body></html>'

          # First response: redirect
          redirect_response = mock('redirect_response')
          redirect_response.stubs(:is_a?).with(Net::HTTPSuccess).returns(false)
          redirect_response.stubs(:is_a?).with(Net::HTTPRedirection).returns(true)
          redirect_response.stubs(:[]).with('location').returns('https://example.com/final')
          redirect_response.stubs(:body).returns('redirect')

          # Second response: success
          success_response = mock('success_response')
          success_response.stubs(:body).returns(html_content)
          success_response.stubs(:[]).with('content-type').returns('text/html')
          success_response.stubs(:is_a?).with(Net::HTTPSuccess).returns(true)
          success_response.stubs(:is_a?).with(Net::HTTPRedirection).returns(false)

          mock_http1 = mock('http1')
          mock_http1.stubs(:use_ssl=)
          mock_http1.stubs(:open_timeout=)
          mock_http1.stubs(:read_timeout=)
          mock_http1.stubs(:request).returns(redirect_response)

          mock_http2 = mock('http2')
          mock_http2.stubs(:use_ssl=)
          mock_http2.stubs(:open_timeout=)
          mock_http2.stubs(:read_timeout=)
          mock_http2.stubs(:request).returns(success_response)

          Net::HTTP.stubs(:new).with('example.com', 443).returns(mock_http1, mock_http2)

          result = @webfetch_tool.call(url: 'https://example.com/redirect')

          assert result[:success]
          assert_equal 'https://example.com/final', result[:url]
        end

        def test_webfetch_max_redirects_exceeded
          # Create 6 redirect responses (more than max 5)
          redirect_responses = 6.times.map do |i|
            resp = mock("redirect_response_#{i}")
            resp.stubs(:is_a?).with(Net::HTTPSuccess).returns(false)
            resp.stubs(:is_a?).with(Net::HTTPRedirection).returns(true)
            resp.stubs(:[]).with('location').returns("https://example.com/#{i + 1}")
            resp.stubs(:body).returns('redirect')
            resp
          end

          mock_https = redirect_responses.map do |resp|
            http = mock('http')
            http.stubs(:use_ssl=)
            http.stubs(:open_timeout=)
            http.stubs(:read_timeout=)
            http.stubs(:request).returns(resp)
            http
          end

          Net::HTTP.stubs(:new).returns(*mock_https)

          result = @webfetch_tool.call(url: 'https://example.com/start')

          refute result[:success]
          assert_equal 'Failed to fetch content', result[:error]
        end

        def test_webfetch_invalid_url
          result = @webfetch_tool.call(url: 'not-a-url')

          refute result[:success]
          assert_includes result[:error], 'Invalid URL'
        end

        def test_webfetch_non_http_url
          result = @webfetch_tool.call(url: 'ftp://example.com')

          refute result[:success]
          assert_includes result[:error], 'Invalid HTTP URL'
        end

        def test_webfetch_timeout
          mock_http = mock('http')
          mock_http.stubs(:use_ssl=)
          mock_http.stubs(:open_timeout=)
          mock_http.stubs(:read_timeout=)
          mock_http.stubs(:request).raises(Net::ReadTimeout.new('timeout'))

          Net::HTTP.stubs(:new).returns(mock_http)

          result = @webfetch_tool.call(url: 'https://example.com')

          refute result[:success]
          assert_includes result[:error], 'Request timeout'
        end

        def test_webfetch_network_error
          mock_http = mock('http')
          mock_http.stubs(:use_ssl=)
          mock_http.stubs(:open_timeout=)
          mock_http.stubs(:read_timeout=)
          mock_http.stubs(:request).raises(StandardError.new('network error'))

          Net::HTTP.stubs(:new).returns(mock_http)

          result = @webfetch_tool.call(url: 'https://example.com')

          refute result[:success]
          assert_equal 'network error', result[:error]
        end

        def test_webfetch_invalid_format
          result = @webfetch_tool.call(url: 'https://example.com', format: 'invalid')

          refute result[:success]
          assert_includes result[:error], 'Unsupported format'
        end

        def test_webfetch_timeout_validation
          html_content = '<html><body>Test</body></html>'
          mock_response = mock('response')
          mock_response.stubs(:body).returns(html_content)
          mock_response.stubs(:[]).with('content-type').returns('text/html')
          mock_response.stubs(:is_a?).with(Net::HTTPSuccess).returns(true)
          mock_response.stubs(:is_a?).with(Net::HTTPRedirection).returns(false)

          mock_http = mock('http')
          mock_http.stubs(:use_ssl=)
          mock_http.stubs(:open_timeout=).with(120) # Should be clamped to 120
          mock_http.stubs(:read_timeout=).with(120)
          mock_http.stubs(:request).returns(mock_response)

          Net::HTTP.stubs(:new).returns(mock_http)

          result = @webfetch_tool.call(url: 'https://example.com', timeout: 200)

          assert result[:success]
        end

        def test_webfetch_timeout_minimum
          html_content = '<html><body>Test</body></html>'
          mock_response = mock('response')
          mock_response.stubs(:body).returns(html_content)
          mock_response.stubs(:[]).with('content-type').returns('text/html')
          mock_response.stubs(:is_a?).with(Net::HTTPSuccess).returns(true)
          mock_response.stubs(:is_a?).with(Net::HTTPRedirection).returns(false)

          mock_http = mock('http')
          mock_http.stubs(:use_ssl=)
          mock_http.stubs(:open_timeout=).with(1) # Should be clamped to 1
          mock_http.stubs(:read_timeout=).with(1)
          mock_http.stubs(:request).returns(mock_response)

          Net::HTTP.stubs(:new).returns(mock_http)

          result = @webfetch_tool.call(url: 'https://example.com', timeout: 0)

          assert result[:success]
        end

        def test_webfetch_empty_redirect_location
          redirect_response = mock('redirect_response')
          redirect_response.stubs(:is_a?).with(Net::HTTPSuccess).returns(false)
          redirect_response.stubs(:is_a?).with(Net::HTTPRedirection).returns(true)
          redirect_response.stubs(:[]).with('location').returns('')
          redirect_response.stubs(:body).returns('redirect')

          mock_http = mock('http')
          mock_http.stubs(:use_ssl=)
          mock_http.stubs(:open_timeout=)
          mock_http.stubs(:read_timeout=)
          mock_http.stubs(:request).returns(redirect_response)

          Net::HTTP.stubs(:new).returns(mock_http)

          result = @webfetch_tool.call(url: 'https://example.com')

          refute result[:success]
          assert_equal 'Failed to fetch content', result[:error]
        end

        def test_webfetch_invalid_redirect_location
          redirect_response = mock('redirect_response')
          redirect_response.stubs(:is_a?).with(Net::HTTPSuccess).returns(false)
          redirect_response.stubs(:is_a?).with(Net::HTTPRedirection).returns(true)
          redirect_response.stubs(:[]).with('location').returns('::invalid::')
          redirect_response.stubs(:body).returns('redirect')

          mock_http = mock('http')
          mock_http.stubs(:use_ssl=)
          mock_http.stubs(:open_timeout=)
          mock_http.stubs(:read_timeout=)
          mock_http.stubs(:request).returns(redirect_response)

          Net::HTTP.stubs(:new).returns(mock_http)

          result = @webfetch_tool.call(url: 'https://example.com')

          refute result[:success]
          assert_equal 'Failed to fetch content', result[:error]
        end

        def test_webfetch_markdown_conversion_with_lists
          html_content = '<html><body><h1>Title</h1><ul><li>Item 1</li><li>Item 2</li></ul></body></html>'
          mock_response = mock('response')
          mock_response.stubs(:body).returns(html_content)
          mock_response.stubs(:[]).with('content-type').returns('text/html')
          mock_response.stubs(:is_a?).with(Net::HTTPSuccess).returns(true)
          mock_response.stubs(:is_a?).with(Net::HTTPRedirection).returns(false)

          mock_http = mock('http')
          mock_http.stubs(:use_ssl=)
          mock_http.stubs(:open_timeout=)
          mock_http.stubs(:read_timeout=)
          mock_http.stubs(:request).returns(mock_response)

          Net::HTTP.stubs(:new).returns(mock_http)

          result = @webfetch_tool.call(url: 'https://example.com', format: 'markdown')

          assert result[:success]
          assert_includes result[:content], '# Title'
          assert_includes result[:content], '- Item 1'
          assert_includes result[:content], '- Item 2'
        end

        def test_webfetch_markdown_conversion_with_links
          html_content = '<html><body><p>Check <a href="https://example.com">this link</a></p></body></html>'
          mock_response = mock('response')
          mock_response.stubs(:body).returns(html_content)
          mock_response.stubs(:[]).with('content-type').returns('text/html')
          mock_response.stubs(:is_a?).with(Net::HTTPSuccess).returns(true)
          mock_response.stubs(:is_a?).with(Net::HTTPRedirection).returns(false)

          mock_http = mock('http')
          mock_http.stubs(:use_ssl=)
          mock_http.stubs(:open_timeout=)
          mock_http.stubs(:read_timeout=)
          mock_http.stubs(:request).returns(mock_response)

          Net::HTTP.stubs(:new).returns(mock_http)

          result = @webfetch_tool.call(url: 'https://example.com', format: 'markdown')

          assert result[:success]
          assert_includes result[:content], '[this link](https://example.com)'
        end
      end
    end
  end
end
