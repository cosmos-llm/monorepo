/**
 * HTTP client implementation for cosmos-llm.
 *
 * Provides a unified interface for making HTTP requests to LLM APIs,
 * with support for both regular and streaming responses.
 */

import fetch, { Response as FetchResponse } from 'node-fetch';

export interface HttpHeaders {
  [key: string]: string;
}

export interface HttpResponse {
  status: number;
  body: any;
}

export interface StreamChunk {
  [key: string]: any;
}

export interface StreamOptions {
  onChunk: (chunk: StreamChunk) => void;
  headers: HttpHeaders;
  body: any;
}

/**
 * HTTP client for making requests to LLM APIs.
 */
export class HttpClient {
  private baseUrl: string;

  constructor(baseUrl: string) {
    this.baseUrl = baseUrl;
  }

  /**
   * Performs a GET request.
   *
   * @param path - The API endpoint path
   * @param headers - Optional HTTP headers
   * @returns The response object
   */
  async get(path: string, headers: HttpHeaders = {}): Promise<HttpResponse> {
    const url = `${this.baseUrl}/${path}`;
    const response = await fetch(url, {
      method: 'GET',
      headers: {
        'Content-Type': 'application/json',
        ...headers,
      },
    });

    const body = await this.parseResponse(response);
    return {
      status: response.status,
      body,
    };
  }

  /**
   * Performs a POST request.
   *
   * @param path - The API endpoint path
   * @param body - The request body
   * @param headers - Optional HTTP headers
   * @returns The response object
   */
  async post(
    path: string,
    body: any,
    headers: HttpHeaders = {}
  ): Promise<HttpResponse> {
    const url = `${this.baseUrl}/${path}`;
    const response = await fetch(url, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        ...headers,
      },
      body: JSON.stringify(body),
    });

    const responseBody = await this.parseResponse(response);
    return {
      status: response.status,
      body: responseBody,
    };
  }

  /**
   * Performs a streaming POST request.
   *
   * This method handles Server-Sent Events (SSE) streaming responses,
   * parsing individual JSON events and calling the provided callback with each chunk.
   *
   * @param path - The API endpoint path
   * @param options - Streaming options including headers, body, and chunk handler
   * @returns The final response object
   */
  async postStream(path: string, options: StreamOptions): Promise<HttpResponse> {
    const url = `${this.baseUrl}/${path}`;
    const response = await fetch(url, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Accept: 'text/event-stream',
        ...options.headers,
      },
      body: JSON.stringify(options.body),
    });

    if (!response.ok) {
      const errorBody = await this.parseResponse(response);
      return {
        status: response.status,
        body: errorBody,
      };
    }

    if (!response.body) {
      throw new Error('Response body is null');
    }

    // Process the stream
    const reader = response.body;
    let buffer = '';

    for await (const chunk of reader) {
      buffer += chunk.toString();
      const lines = buffer.split('\n');

      // Keep the last incomplete line in the buffer
      buffer = lines.pop() || '';

      for (const line of lines) {
        if (line.startsWith('data: ')) {
          const data = line.slice(6);
          if (data === '[DONE]') {
            continue;
          }

          try {
            const parsed = JSON.parse(data);
            options.onChunk(parsed);
          } catch (e) {
            // Skip malformed JSON chunks
          }
        }
      }
    }

    return {
      status: response.status,
      body: {},
    };
  }

  /**
   * Parses the response body as JSON.
   *
   * @param response - The fetch response
   * @returns The parsed JSON body
   */
  private async parseResponse(response: FetchResponse): Promise<any> {
    const text = await response.text();
    if (!text) {
      return {};
    }

    try {
      return JSON.parse(text);
    } catch (e) {
      return text;
    }
  }
}
