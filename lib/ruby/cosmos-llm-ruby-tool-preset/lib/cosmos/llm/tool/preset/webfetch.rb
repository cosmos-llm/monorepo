# frozen_string_literal: true

require 'cosmos/llm/tool'
require 'net/http'
require 'uri'

begin
  require 'nokogiri'
  NOKOGIRI_AVAILABLE = true
rescue LoadError
  NOKOGIRI_AVAILABLE = false
end

module Cosmos
  module Llm
    module Tool
      module Preset
        # Web content fetching preset tool
        #
        # Provides functionality to fetch content from URLs with support for
        # HTML to markdown conversion and timeout handling.
        #
        # @example Using the webfetch tool
        #   tool = Cosmos::Llm::Tool::Preset.webfetch
        #   result = tool.call(url: 'https://example.com', format: 'markdown')
        #
        # @return [Cosmos::Llm::Tool::Definition] A webfetch tool
        def self.webfetch
          Cosmos::Llm::Tool.define(:webfetch, register: false) do
            description 'Fetch content from URLs with format conversion support (text, markdown, html)'

            parameter :url,
                      type: :string,
                      required: true,
                      description: 'The URL to fetch content from'

            parameter :format,
                      type: :string,
                      enum: %w[text markdown html],
                      required: false,
                      description: 'The format to return content in (default: markdown)'

            parameter :timeout,
                      type: :number,
                      required: false,
                      description: 'Optional timeout in seconds (max 120, default: 30)'

            execute do |params|
              url = params[:url]
              format = params.fetch(:format, 'markdown')
              timeout = params.fetch(:timeout, 30).to_i

              # Validate timeout
              timeout = [[timeout, 1].max, 120].min

              begin
                uri = URI.parse(url)
                unless uri.is_a?(URI::HTTP) || uri.is_a?(URI::HTTPS)
                  raise URI::InvalidURIError, "Invalid HTTP URL: #{url}"
                end

                # Upgrade HTTP to HTTPS
                if uri.scheme == 'http'
                  uri.scheme = 'https'
                  uri.port = 443 if uri.port == 80
                end

                # Validate format
                raise ArgumentError, "Unsupported format: #{format}" unless %w[html text markdown].include?(format)

                content, = fetch_with_redirects(uri, timeout)

                raise StandardError, 'Failed to fetch content' unless content

                # Convert content based on format
                case format
                when 'html'
                  content
                when 'text'
                  html_to_text(content)
                when 'markdown'
                  html_to_markdown(content)
                end

                # Validate format
                raise ArgumentError, "Unsupported format: #{format}" unless %w[html text markdown].include?(format)

                content, final_uri, content_type = fetch_with_redirects(uri, timeout)

                raise StandardError, 'Failed to fetch content' unless content

                # Convert content based on format
                processed_content = case format
                                    when 'html'
                                      content
                                    when 'text'
                                      html_to_text(content)
                                    when 'markdown'
                                      html_to_markdown(content)
                                    end

                {
                  success: true,
                  url: final_uri.to_s,
                  format: format,
                  content: processed_content,
                  content_type: content_type,
                  size: processed_content.bytesize,
                  fetched_at: Time.now.iso8601
                }
              rescue URI::InvalidURIError => e
                {
                  success: false,
                  error: "Invalid URL: #{e.message}",
                  url: url
                }
              rescue Net::OpenTimeout, Net::ReadTimeout => e
                {
                  success: false,
                  error: "Request timeout: #{e.message}",
                  url: url
                }
              rescue ArgumentError => e
                {
                  success: false,
                  error: e.message,
                  url: url
                }
              rescue StandardError => e
                {
                  success: false,
                  error: e.message,
                  url: url
                }
              end
            end
          end
        end

        # Fetch content with redirect handling (helper method)
        # @api private
        def self.fetch_with_redirects(uri, timeout, max_redirects = 5)
          current_uri = uri
          redirect_count = 0

          while redirect_count <= max_redirects
            http = Net::HTTP.new(current_uri.host, current_uri.port)
            http.use_ssl = (current_uri.scheme == 'https')
            http.open_timeout = timeout
            http.read_timeout = timeout

            request = Net::HTTP::Get.new(current_uri.request_uri)
            request['User-Agent'] = 'Durable-LLM-Tool-Preset/1.0'
            response = http.request(request)

            if response.is_a?(Net::HTTPSuccess)
              return [response.body, current_uri, response['content-type']]
            elsif response.is_a?(Net::HTTPRedirection)
              location = response['location']
              return [nil, nil, nil] unless location && !location.strip.empty?

              begin
                new_uri = URI.parse(location)
                new_uri = current_uri.merge(new_uri) if new_uri.relative?
                current_uri = new_uri
                redirect_count += 1
              rescue URI::InvalidURIError
                return [nil, nil, nil]
              end
            else
              return [nil, nil, nil]
            end
          end

          # Too many redirects
          [nil, nil, nil]
        end

        # Convert HTML to plain text (helper method)
        # @api private
        def self.html_to_text(html)
          if NOKOGIRI_AVAILABLE
            doc = Nokogiri::HTML(html)
            doc.text.strip
          else
            # Basic HTML tag removal without nokogiri
            html.gsub(/<[^>]+>/, '').strip
          end
        end

        # Convert HTML to markdown (helper method)
        # @api private
        def self.html_to_markdown(html)
          if NOKOGIRI_AVAILABLE
            doc = Nokogiri::HTML(html)
            content = extract_text_with_formatting(doc)
            content.gsub(/\n{3,}/, "\n\n").strip
          else
            # Basic HTML tag removal without nokogiri
            html.gsub(/<[^>]+>/, '').strip
          end
        end

        # Extract text with markdown formatting (helper method)
        # @api private
        def self.extract_text_with_formatting(node, _level = 0)
          return '' unless node

          case node.name.downcase
          when 'text'
            node.text
          when 'h1'
            "# #{node.text.strip}\n\n"
          when 'h2'
            "## #{node.text.strip}\n\n"
          when 'h3'
            "### #{node.text.strip}\n\n"
          when 'h4'
            "#### #{node.text.strip}\n\n"
          when 'h5'
            "##### #{node.text.strip}\n\n"
          when 'h6'
            "###### #{node.text.strip}\n\n"
          when 'p'
            "#{node.children.map { |child| extract_text_with_formatting(child) }.join}\n\n"
          when 'br'
            "\n"
          when 'strong', 'b'
            "**#{node.children.map { |child| extract_text_with_formatting(child) }.join}**"
          when 'em', 'i'
            "*#{node.children.map { |child| extract_text_with_formatting(child) }.join}*"
          when 'a'
            href = node['href']
            text = node.children.map { |child| extract_text_with_formatting(child) }.join
            "[#{text}](#{href})"
          when 'ul', 'ol'
            list_items = node.css('li').map do |li|
              text = li.children.map { |child| extract_text_with_formatting(child) }.join
              "  - #{text}"
            end
            "#{list_items.join("\n")}\n\n"
          when 'li'
            text = node.children.map { |child| extract_text_with_formatting(child) }.join
            "- #{text}"
          when 'div', 'span', 'section', 'article'
            node.children.map { |child| extract_text_with_formatting(child) }.join
          else
            node.children.map { |child| extract_text_with_formatting(child) }.join
          end
        end
      end
    end
  end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
