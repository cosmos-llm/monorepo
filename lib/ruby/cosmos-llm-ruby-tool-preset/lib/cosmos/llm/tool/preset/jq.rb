# frozen_string_literal: true

require 'cosmos/llm/tool'
require 'json'

module Cosmos
  module Llm
    module Tool
      module Preset
        def self.jq(filesystem = nil)
          Cosmos::Llm::Tool.define(:jq, register: false) do
            description 'Query and transform JSON data using jq-style queries'

            parameter :json,
                       type: :string,
                       required: false,
                       description: 'JSON string to query (required if file_path not provided)'

            parameter :file_path,
                       type: :string,
                       required: false,
                       description: 'Path to JSON file in virtual filesystem (required if json not provided)'

            parameter :query,
                       type: :string,
                       required: true,
                       description: 'JQ-style query (e.g., ".", ".name", ".items[]", ".items[0]", "keys", "length")'

            parameter :compact,
                       type: :boolean,
                       required: false,
                       description: 'Output compact JSON (default: false)'

              execute do |params|
                json_input = params[:json]
                file_path = params[:file_path]
                query = params[:query]
                compact = params.fetch(:compact, false)

                if !json_input && !file_path
                  {
                    success: false,
                    error: 'Either json or file_path parameter is required'
                  }
                elsif file_path && !filesystem
                  {
                    success: false,
                    error: 'Filesystem not provided but file_path specified'
                  }
                elsif file_path
                  virtual_file = filesystem.find_file(file_path)
                  if !virtual_file
                    {
                      success: false,
                      error: 'File not found in virtual filesystem',
                      file_path: file_path
                    }
                  else
                    json_input = virtual_file.content
                    source = "file:#{file_path}"
                    data = nil
                    json_error = nil
                    begin
                      data = JSON.parse(json_input)
                    rescue JSON::ParserError => e
                      json_error = "Invalid JSON: #{e.message}"
                    end
                    if json_error
                      {
                        success: false,
                        error: json_error
                      }
                    else
                      result = nil
                      query_error = nil
                      begin
                        query_stripped = query.strip
                        result = case query_stripped
                                 when '.'
                                   data
                                 when 'keys'
                                   if data.is_a?(Hash)
                                     data.keys.sort
                                   elsif data.is_a?(Array)
                                     (0...data.length).to_a
                                   else
                                     raise "Cannot get keys of #{data.class}"
                                   end
                                 when 'values'
                                   if data.is_a?(Hash)
                                     data.values
                                   elsif data.is_a?(Array)
                                     data
                                   else
                                     raise "Cannot get values of #{data.class}"
                                   end
                                 when 'length'
                                   if data.respond_to?(:length)
                                     data.length
                                   else
                                     raise "Cannot get length of #{data.class}"
                                   end
                                 when 'type'
                                   if data.is_a?(Hash)
                                     'object'
                                   elsif data.is_a?(Array)
                                     'array'
                                   elsif data.is_a?(String)
                                     'string'
                                   elsif data.is_a?(Integer) || data.is_a?(Float)
                                     'number'
                                   elsif data.is_a?(TrueClass) || data.is_a?(FalseClass)
                                     'boolean'
                                   elsif data.nil?
                                     'null'
                                   else
                                     'unknown'
                                   end
                                 else
                                   current = data
                                   query_stripped = query_stripped[1..] if query_stripped.start_with?('.')
                                   parts = []
                                   current_part = ''
                                   bracket_depth = 0
                                   query_stripped.each_char do |char|
                                     case char
                                     when '['
                                       if bracket_depth == 0
                                         parts << current_part unless current_part.empty?
                                         current_part = ''
                                       end
                                       current_part += char
                                       bracket_depth += 1
                                     when ']'
                                       current_part += char
                                       bracket_depth -= 1
                                       if bracket_depth == 0
                                         parts << current_part
                                         current_part = ''
                                       end
                                     when '.'
                                       if bracket_depth == 0
                                         parts << current_part unless current_part.empty?
                                         current_part = ''
                                       else
                                         current_part += char
                                       end
                                     else
                                       current_part += char
                                     end
                                   end
                                   parts << current_part unless current_part.empty?
                                   parts = parts.reject(&:empty?)
                                   parts.each do |part|
                                     next if current.nil?
                                     if part.start_with?('[') && part.end_with?(']')
                                       index_str = part[1...-1]
                                       if index_str.empty?
                                         unless current.is_a?(Array)
                                           raise "Cannot iterate over non-array: #{current.class}"
                                         end
                                       else
                                         begin
                                           index = Integer(index_str)
                                         rescue ArgumentError
                                           raise "Invalid array index: #{index_str}"
                                         end
                                         unless current.is_a?(Array)
                                           raise "Cannot index non-array with [#{index}]"
                                         end
                                         current = current[index]
                                       end
                                     else
                                       unless current.is_a?(Hash)
                                         raise "Cannot access key '#{part}' on non-object: #{current.class}"
                                       end
                                       current = current[part]
                                     end
                                   end
                                   current
                                 end
                      rescue StandardError => e
                        query_error = e.message
                      end
                      if query_error
                        {
                          success: false,
                          error: query_error,
                          query: query
                        }
                      else
                        output = if compact
                                   JSON.generate(result)
                                 else
                                   JSON.pretty_generate(result)
                                 end
                        {
                          success: true,
                          query: query,
                          result: result,
                          output: output,
                          source: source
                        }
                      end
                    end
                  end
                else
                  source = 'string'
                  data = nil
                  json_error = nil
                  begin
                    data = JSON.parse(json_input)
                  rescue JSON::ParserError => e
                    json_error = "Invalid JSON: #{e.message}"
                  end
                  if json_error
                    {
                      success: false,
                      error: json_error
                    }
                  else
                    result = nil
                    query_error = nil
                    begin
                      query_stripped = query.strip
                      result = case query_stripped
                               when '.'
                                 data
                               when 'keys'
                                 if data.is_a?(Hash)
                                   data.keys.sort
                                 elsif data.is_a?(Array)
                                   (0...data.length).to_a
                                 else
                                   raise "Cannot get keys of #{data.class}"
                                 end
                               when 'values'
                                 if data.is_a?(Hash)
                                   data.values
                                 elsif data.is_a?(Array)
                                   data
                                 else
                                   raise "Cannot get values of #{data.class}"
                                 end
                               when 'length'
                                 if data.respond_to?(:length)
                                   data.length
                                 else
                                   raise "Cannot get length of #{data.class}"
                                 end
                               when 'type'
                                 if data.is_a?(Hash)
                                   'object'
                                 elsif data.is_a?(Array)
                                   'array'
                                 elsif data.is_a?(String)
                                   'string'
                                 elsif data.is_a?(Integer) || data.is_a?(Float)
                                   'number'
                                 elsif data.is_a?(TrueClass) || data.is_a?(FalseClass)
                                   'boolean'
                                 elsif data.nil?
                                   'null'
                                 else
                                   'unknown'
                                 end
                               else
                                 current = data
                                 query_stripped = query_stripped[1..] if query_stripped.start_with?('.')
                                 parts = []
                                 current_part = ''
                                 bracket_depth = 0
                                 query_stripped.each_char do |char|
                                   case char
                                   when '['
                                     if bracket_depth == 0
                                       parts << current_part unless current_part.empty?
                                       current_part = ''
                                     end
                                     current_part += char
                                     bracket_depth += 1
                                   when ']'
                                     current_part += char
                                     bracket_depth -= 1
                                     if bracket_depth == 0
                                       parts << current_part
                                       current_part = ''
                                     end
                                   when '.'
                                     if bracket_depth == 0
                                       parts << current_part unless current_part.empty?
                                       current_part = ''
                                     else
                                       current_part += char
                                     end
                                   else
                                     current_part += char
                                   end
                                 end
                                 parts << current_part unless current_part.empty?
                                 parts = parts.reject(&:empty?)
                                 parts.each do |part|
                                   next if current.nil?
                                   if part.start_with?('[') && part.end_with?(']')
                                     index_str = part[1...-1]
                                     if index_str.empty?
                                       unless current.is_a?(Array)
                                         raise "Cannot iterate over non-array: #{current.class}"
                                       end
                                     else
                                       begin
                                         index = Integer(index_str)
                                       rescue ArgumentError
                                         raise "Invalid array index: #{index_str}"
                                       end
                                       unless current.is_a?(Array)
                                         raise "Cannot index non-array with [#{index}]"
                                       end
                                       current = current[index]
                                     end
                                   else
                                     unless current.is_a?(Hash)
                                       raise "Cannot access key '#{part}' on non-object: #{current.class}"
                                     end
                                     current = current[part]
                                   end
                                 end
                                 current
                               end
                    rescue StandardError => e
                      query_error = e.message
                    end
                    if query_error
                      {
                        success: false,
                        error: query_error,
                        query: query
                      }
                    else
                      output = if compact
                                 JSON.generate(result)
                               else
                                 JSON.pretty_generate(result)
                               end
                      {
                        success: true,
                        query: query,
                        result: result,
                        output: output,
                        source: source
                      }
                    end
                  end
                end
              end
       end
     end
   end
end

# Copyright (c) 2025 Durable Programming, LLC. All rights reserved.
