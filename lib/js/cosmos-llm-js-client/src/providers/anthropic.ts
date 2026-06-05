/**
 * Anthropic provider implementation.
 *
 * Provides access to Anthropic's Claude models including Claude 3.5 Sonnet,
 * Claude 3 Opus, and Claude 3 Haiku. Supports message-based completions and streaming.
 */

import { HttpClient } from '../http-client';
import { BaseProvider, CompletionOptions, StreamOptions } from './base';
import {
  AuthenticationError,
  RateLimitError,
  InvalidRequestError,
  ServerError,
  APIError,
} from '../errors';

const BASE_URL = 'https://api.anthropic.com';

export interface AnthropicOptions {
  apiKey?: string;
}

/**
 * Anthropic provider for accessing Claude language models.
 */
export class AnthropicProvider extends BaseProvider {
  private client: HttpClient;

  constructor(options: AnthropicOptions = {}) {
    super(options.apiKey);
    this.client = new HttpClient(BASE_URL);
  }

  protected getDefaultApiKey(): string | undefined {
    return process.env.ANTHROPIC_API_KEY || process.env.CLLM__ANTHROPIC__API_KEY;
  }

  /**
   * Performs a message completion request.
   *
   * @param options - The completion options
   * @returns The completion response
   */
  async completion(options: CompletionOptions): Promise<any> {
    this.validateApiKey();
    this.validateRequiredParams(options, ['model', 'messages']);

    const { systemMessage, messages } = this.extractSystemMessage(options.messages);
    const body = this.buildRequestBody(options, messages, systemMessage);

    const headers = {
      'x-api-key': this.apiKey!,
      'anthropic-version': '2023-06-01',
    };

    const response = await this.client.post('/v1/messages', body, headers);
    return this.handleResponse(response);
  }

  /**
   * Performs a streaming message completion request.
   *
   * @param options - The streaming options including onChunk callback
   */
  async stream(options: StreamOptions): Promise<void> {
    this.validateApiKey();
    this.validateRequiredParams(options, ['model', 'messages']);

    const { systemMessage, messages } = this.extractSystemMessage(options.messages);
    const body = {
      ...this.buildRequestBody(options, messages, systemMessage),
      stream: true,
    };

    const headers = {
      'x-api-key': this.apiKey!,
      'anthropic-version': '2023-06-01',
    };

    const response = await this.client.postStream('/v1/messages', {
      headers,
      body,
      onChunk: (chunk) => {
        // Only emit content_block_delta events with text
        if (chunk.type === 'content_block_delta' && chunk.delta?.text) {
          options.onChunk(chunk);
        }
      },
    });

    if (response.status !== 200) {
      this.handleResponse(response);
    }
  }

  /**
   * Retrieves the list of supported Claude models.
   *
   * @returns Array of model identifiers
   */
  async models(): Promise<string[]> {
    return [
      'claude-3-5-sonnet-20240620',
      'claude-3-opus-20240229',
      'claude-3-haiku-20240307',
    ];
  }

  /**
   * Indicates that this provider supports streaming.
   *
   * @returns True
   */
  supportsStreaming(): boolean {
    return true;
  }

  /**
   * Extracts the system message from messages array.
   * Anthropic expects system messages as a top-level parameter.
   *
   * @param messages - The messages array
   * @returns Object with systemMessage and filtered messages
   */
  private extractSystemMessage(
    messages: Array<{ role: string; content: string; [key: string]: any }>
  ): { systemMessage?: string; messages: typeof messages } {
    if (messages.length > 0 && messages[0].role === 'system') {
      return {
        systemMessage: messages[0].content,
        messages: messages.slice(1),
      };
    }
    return { messages };
  }

  /**
   * Builds the request body for a completion request.
   *
   * @param options - The completion options
   * @param messages - The messages array (without system message)
   * @param systemMessage - The optional system message
   * @returns The request body
   */
  private buildRequestBody(
    options: CompletionOptions,
    messages: Array<{ role: string; content: string; [key: string]: any }>,
    systemMessage?: string
  ): any {
    const body: any = {
      model: options.model,
      messages,
      max_tokens: options.maxTokens || 1024,
    };

    if (systemMessage) {
      body.system = systemMessage;
    }

    if (options.temperature !== undefined) {
      body.temperature = options.temperature;
    }
    if (options.topP !== undefined) {
      body.top_p = options.topP;
    }
    if (options.stop !== undefined) {
      body.stop_sequences = Array.isArray(options.stop) ? options.stop : [options.stop];
    }

    // Include any additional options
    for (const key in options) {
      if (
        ![
          'model',
          'messages',
          'temperature',
          'maxTokens',
          'topP',
          'stop',
          'onChunk',
          'frequencyPenalty',
          'presencePenalty',
        ].includes(key)
      ) {
        body[key] = options[key];
      }
    }

    return body;
  }

  /**
   * Handles the API response and throws appropriate errors.
   *
   * @param response - The HTTP response
   * @returns The response body for successful responses
   * @throws Error for failed responses
   */
  private handleResponse(response: { status: number; body: any }): any {
    const { status, body } = response;

    if (status >= 200 && status < 300) {
      return body;
    }

    const errorMessage = this.parseErrorMessage(body);

    switch (status) {
      case 401:
        throw new AuthenticationError(errorMessage);
      case 429:
        throw new RateLimitError(errorMessage);
      case 400:
      case 404:
        throw new InvalidRequestError(errorMessage);
      default:
        if (status >= 500) {
          throw new ServerError(errorMessage);
        }
        throw new APIError(`Unexpected response code: ${status}`);
    }
  }

  /**
   * Parses the error message from the response body.
   *
   * @param body - The response body
   * @returns The formatted error message
   */
  private parseErrorMessage(body: any): string {
    if (typeof body === 'string') {
      return body;
    }

    if (body?.error?.message) {
      return body.error.message;
    }

    return JSON.stringify(body);
  }
}
