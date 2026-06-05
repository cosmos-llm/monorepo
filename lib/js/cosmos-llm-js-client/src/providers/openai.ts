/**
 * OpenAI provider implementation.
 *
 * Provides access to OpenAI's language models including GPT-3.5, GPT-4,
 * and embeddings models. Supports chat completions, embeddings, and streaming.
 */

import { HttpClient } from '../http-client';
import { BaseProvider, CompletionOptions, EmbeddingOptions, StreamOptions } from './base';
import {
  AuthenticationError,
  RateLimitError,
  InvalidRequestError,
  ServerError,
  APIError,
} from '../errors';

const BASE_URL = 'https://api.openai.com/v1';

export interface OpenAIOptions {
  apiKey?: string;
  organization?: string;
}

/**
 * OpenAI provider for accessing OpenAI's language models.
 */
export class OpenAIProvider extends BaseProvider {
  private client: HttpClient;
  private organization?: string;

  constructor(options: OpenAIOptions = {}) {
    super(options.apiKey);
    this.organization = options.organization || process.env.OPENAI_ORGANIZATION;
    this.client = new HttpClient(BASE_URL);
  }

  protected getDefaultApiKey(): string | undefined {
    return process.env.OPENAI_API_KEY || process.env.CLLM__OPENAI__API_KEY;
  }

  /**
   * Performs a chat completion request.
   *
   * @param options - The completion options
   * @returns The completion response
   */
  async completion(options: CompletionOptions): Promise<any> {
    this.validateApiKey();
    this.validateRequiredParams(options, ['model', 'messages']);

    const headers: any = {
      Authorization: `Bearer ${this.apiKey}`,
    };

    if (this.organization) {
      headers['OpenAI-Organization'] = this.organization;
    }

    const body = this.buildRequestBody(options);
    const response = await this.client.post('chat/completions', body, headers);

    return this.handleResponse(response);
  }

  /**
   * Performs a streaming chat completion request.
   *
   * @param options - The streaming options including onChunk callback
   */
  async stream(options: StreamOptions): Promise<void> {
    this.validateApiKey();
    this.validateRequiredParams(options, ['model', 'messages']);

    const headers: any = {
      Authorization: `Bearer ${this.apiKey}`,
    };

    if (this.organization) {
      headers['OpenAI-Organization'] = this.organization;
    }

    const body = {
      ...this.buildRequestBody(options),
      stream: true,
    };

    const response = await this.client.postStream('chat/completions', {
      headers,
      body,
      onChunk: options.onChunk,
    });

    if (response.status !== 200) {
      this.handleResponse(response);
    }
  }

  /**
   * Generates embeddings for the given text.
   *
   * @param options - The embedding options
   * @returns The embedding response
   */
  async embedding(options: EmbeddingOptions): Promise<any> {
    this.validateApiKey();
    this.validateRequiredParams(options, ['model', 'input']);

    const headers = {
      Authorization: `Bearer ${this.apiKey}`,
      ...(this.organization ? { 'OpenAI-Organization': this.organization } : {}),
    };

    const response = await this.client.post('embeddings', options, headers);
    return this.handleResponse(response);
  }

  /**
   * Retrieves the list of available models.
   *
   * @returns Array of model IDs
   */
  async models(): Promise<string[]> {
    this.validateApiKey();

    const headers = {
      Authorization: `Bearer ${this.apiKey}`,
      ...(this.organization ? { 'OpenAI-Organization': this.organization } : {}),
    };

    const response = await this.client.get('models', headers);
    const result = this.handleResponse(response);

    return result.data.map((model: any) => model.id);
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
   * Builds the request body for a completion request.
   *
   * @param options - The completion options
   * @returns The request body
   */
  private buildRequestBody(options: CompletionOptions): any {
    const body: any = {
      model: options.model,
      messages: options.messages,
    };

    if (options.temperature !== undefined) {
      body.temperature = options.temperature;
    }
    if (options.maxTokens !== undefined) {
      body.max_tokens = options.maxTokens;
    }
    if (options.topP !== undefined) {
      body.top_p = options.topP;
    }
    if (options.frequencyPenalty !== undefined) {
      body.frequency_penalty = options.frequencyPenalty;
    }
    if (options.presencePenalty !== undefined) {
      body.presence_penalty = options.presencePenalty;
    }
    if (options.stop !== undefined) {
      body.stop = options.stop;
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
          'frequencyPenalty',
          'presencePenalty',
          'stop',
          'onChunk',
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
