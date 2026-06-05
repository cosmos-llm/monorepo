/**
 * Unit tests for AnthropicProvider class.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { AnthropicProvider } from './anthropic';
import { HttpClient } from '../http-client';
import {
  AuthenticationError,
  RateLimitError,
  InvalidRequestError,
  ServerError,
} from '../errors';

vi.mock('../http-client', () => ({
  HttpClient: vi.fn(function(this: any) {
    return {
      post: vi.fn(),
      postStream: vi.fn(),
      get: vi.fn(),
    };
  }),
}));

describe('AnthropicProvider', () => {
  let provider: AnthropicProvider;
  let mockHttpClient: any;

  beforeEach(() => {
    vi.clearAllMocks();
    delete process.env.ANTHROPIC_API_KEY;
    delete process.env.CLLM__ANTHROPIC__API_KEY;

    mockHttpClient = {
      post: vi.fn(),
      postStream: vi.fn(),
      get: vi.fn(),
    };

    vi.mocked(HttpClient).mockImplementation(function(this: any) {
      return mockHttpClient;
    });
    provider = new AnthropicProvider({ apiKey: 'test-key' });
  });

  describe('constructor', () => {
    it('should initialize with provided API key', () => {
      expect(provider).toBeDefined();
    });

    it('should use ANTHROPIC_API_KEY from environment', () => {
      process.env.ANTHROPIC_API_KEY = 'env-key';
      const envProvider = new AnthropicProvider();
      expect(envProvider).toBeDefined();
    });

    it('should use CLLM__ANTHROPIC__API_KEY from environment', () => {
      process.env.CLLM__ANTHROPIC__API_KEY = 'cllm-key';
      const envProvider = new AnthropicProvider();
      expect(envProvider).toBeDefined();
    });
  });

  describe('completion', () => {
    it('should make a POST request to /v1/messages endpoint', async () => {
      mockHttpClient.post.mockResolvedValue({
        status: 200,
        body: { content: [{ type: 'text', text: 'Hello!' }] },
      });

      await provider.completion({
        model: 'claude-3-opus-20240229',
        messages: [{ role: 'user', content: 'Hi' }],
      });

      expect(mockHttpClient.post).toHaveBeenCalledWith(
        '/v1/messages',
        expect.objectContaining({
          model: 'claude-3-opus-20240229',
          messages: [{ role: 'user', content: 'Hi' }],
          max_tokens: 1024,
        }),
        expect.objectContaining({
          'x-api-key': 'test-key',
          'anthropic-version': '2023-06-01',
        })
      );
    });

    it('should extract and move system message to top-level parameter', async () => {
      mockHttpClient.post.mockResolvedValue({
        status: 200,
        body: { content: [] },
      });

      await provider.completion({
        model: 'claude-3-opus-20240229',
        messages: [
          { role: 'system', content: 'You are helpful' },
          { role: 'user', content: 'Hi' },
        ],
      });

      expect(mockHttpClient.post).toHaveBeenCalledWith(
        expect.any(String),
        expect.objectContaining({
          system: 'You are helpful',
          messages: [{ role: 'user', content: 'Hi' }],
        }),
        expect.any(Object)
      );
    });

    it('should not include system parameter when no system message', async () => {
      mockHttpClient.post.mockResolvedValue({
        status: 200,
        body: { content: [] },
      });

      await provider.completion({
        model: 'claude-3-opus-20240229',
        messages: [{ role: 'user', content: 'Hi' }],
      });

      const callArgs = mockHttpClient.post.mock.calls[0][1];
      expect(callArgs.system).toBeUndefined();
    });

    it('should include optional parameters in request body', async () => {
      mockHttpClient.post.mockResolvedValue({
        status: 200,
        body: { content: [] },
      });

      await provider.completion({
        model: 'claude-3-opus-20240229',
        messages: [{ role: 'user', content: 'test' }],
        temperature: 0.7,
        maxTokens: 2000,
        topP: 0.9,
        stop: ['STOP'],
      });

      expect(mockHttpClient.post).toHaveBeenCalledWith(
        expect.any(String),
        expect.objectContaining({
          temperature: 0.7,
          max_tokens: 2000,
          top_p: 0.9,
          stop_sequences: ['STOP'],
        }),
        expect.any(Object)
      );
    });

    it('should convert string stop to array', async () => {
      mockHttpClient.post.mockResolvedValue({
        status: 200,
        body: { content: [] },
      });

      await provider.completion({
        model: 'claude-3-opus-20240229',
        messages: [{ role: 'user', content: 'test' }],
        stop: 'STOP',
      });

      const callArgs = mockHttpClient.post.mock.calls[0][1];
      expect(callArgs.stop_sequences).toEqual(['STOP']);
    });

    it('should default max_tokens to 1024', async () => {
      mockHttpClient.post.mockResolvedValue({
        status: 200,
        body: { content: [] },
      });

      await provider.completion({
        model: 'claude-3-opus-20240229',
        messages: [{ role: 'user', content: 'test' }],
      });

      const callArgs = mockHttpClient.post.mock.calls[0][1];
      expect(callArgs.max_tokens).toBe(1024);
    });

    it('should throw AuthenticationError without API key', async () => {
      const noKeyProvider = new AnthropicProvider();
      await expect(
        noKeyProvider.completion({
          model: 'claude-3-opus-20240229',
          messages: [{ role: 'user', content: 'test' }],
        })
      ).rejects.toThrow(AuthenticationError);
    });

    it('should validate required parameters', async () => {
      await expect(
        provider.completion({
          model: 'claude-3-opus-20240229',
          messages: [],
        } as any)
      ).rejects.toThrow('Missing required parameters');
    });

    it('should return response body on success', async () => {
      const responseBody = { content: [{ type: 'text', text: 'Response' }] };
      mockHttpClient.post.mockResolvedValue({
        status: 200,
        body: responseBody,
      });

      const result = await provider.completion({
        model: 'claude-3-opus-20240229',
        messages: [{ role: 'user', content: 'test' }],
      });

      expect(result).toEqual(responseBody);
    });

    it('should throw RateLimitError on 429 status', async () => {
      mockHttpClient.post.mockResolvedValue({
        status: 429,
        body: { error: { message: 'Rate limit exceeded' } },
      });

      await expect(
        provider.completion({
          model: 'claude-3-opus-20240229',
          messages: [{ role: 'user', content: 'test' }],
        })
      ).rejects.toThrow(RateLimitError);
    });

    it('should throw AuthenticationError on 401 status', async () => {
      mockHttpClient.post.mockResolvedValue({
        status: 401,
        body: { error: { message: 'Invalid API key' } },
      });

      await expect(
        provider.completion({
          model: 'claude-3-opus-20240229',
          messages: [{ role: 'user', content: 'test' }],
        })
      ).rejects.toThrow(AuthenticationError);
    });

    it('should throw InvalidRequestError on 400 status', async () => {
      mockHttpClient.post.mockResolvedValue({
        status: 400,
        body: { error: { message: 'Bad request' } },
      });

      await expect(
        provider.completion({
          model: 'claude-3-opus-20240229',
          messages: [{ role: 'user', content: 'test' }],
        })
      ).rejects.toThrow(InvalidRequestError);
    });

    it('should throw ServerError on 500 status', async () => {
      mockHttpClient.post.mockResolvedValue({
        status: 500,
        body: { error: { message: 'Internal server error' } },
      });

      await expect(
        provider.completion({
          model: 'claude-3-opus-20240229',
          messages: [{ role: 'user', content: 'test' }],
        })
      ).rejects.toThrow(ServerError);
    });
  });

  describe('stream', () => {
    it('should make a streaming POST request with stream=true', async () => {
      mockHttpClient.postStream.mockResolvedValue({
        status: 200,
        body: {},
      });

      const onChunk = vi.fn();
      await provider.stream({
        model: 'claude-3-opus-20240229',
        messages: [{ role: 'user', content: 'test' }],
        onChunk,
      });

      expect(mockHttpClient.postStream).toHaveBeenCalledWith(
        '/v1/messages',
        expect.objectContaining({
          headers: expect.objectContaining({
            'x-api-key': 'test-key',
            'anthropic-version': '2023-06-01',
          }),
          body: expect.objectContaining({
            stream: true,
          }),
        })
      );
    });

    it('should filter and forward content_block_delta chunks with text', async () => {
      const capturedOnChunk = vi.fn();
      let streamOnChunk: any;

      mockHttpClient.postStream.mockImplementation(async (_path: string, options: any) => {
        streamOnChunk = options.onChunk;
        return { status: 200, body: {} };
      });

      await provider.stream({
        model: 'claude-3-opus-20240229',
        messages: [{ role: 'user', content: 'test' }],
        onChunk: capturedOnChunk,
      });

      // Simulate various chunk types
      streamOnChunk({ type: 'message_start' });
      streamOnChunk({ type: 'content_block_delta', delta: { text: 'Hello' } });
      streamOnChunk({ type: 'content_block_delta', delta: { text: ' world' } });
      streamOnChunk({ type: 'message_stop' });

      expect(capturedOnChunk).toHaveBeenCalledTimes(2);
      expect(capturedOnChunk).toHaveBeenCalledWith({
        type: 'content_block_delta',
        delta: { text: 'Hello' },
      });
      expect(capturedOnChunk).toHaveBeenCalledWith({
        type: 'content_block_delta',
        delta: { text: ' world' },
      });
    });

    it('should not forward chunks without text delta', async () => {
      const capturedOnChunk = vi.fn();
      let streamOnChunk: any;

      mockHttpClient.postStream.mockImplementation(async (_path: string, options: any) => {
        streamOnChunk = options.onChunk;
        return { status: 200, body: {} };
      });

      await provider.stream({
        model: 'claude-3-opus-20240229',
        messages: [{ role: 'user', content: 'test' }],
        onChunk: capturedOnChunk,
      });

      streamOnChunk({ type: 'content_block_delta', delta: {} });
      streamOnChunk({ type: 'ping' });

      expect(capturedOnChunk).not.toHaveBeenCalled();
    });

    it('should validate API key before streaming', async () => {
      const noKeyProvider = new AnthropicProvider();
      await expect(
        noKeyProvider.stream({
          model: 'claude-3-opus-20240229',
          messages: [{ role: 'user', content: 'test' }],
          onChunk: vi.fn(),
        })
      ).rejects.toThrow(AuthenticationError);
    });

    it('should handle error responses in streaming', async () => {
      mockHttpClient.postStream.mockResolvedValue({
        status: 401,
        body: { error: { message: 'Unauthorized' } },
      });

      await expect(
        provider.stream({
          model: 'claude-3-opus-20240229',
          messages: [{ role: 'user', content: 'test' }],
          onChunk: vi.fn(),
        })
      ).rejects.toThrow(AuthenticationError);
    });
  });

  describe('models', () => {
    it('should return list of supported Claude models', async () => {
      const models = await provider.models();

      expect(models).toEqual([
        'claude-3-5-sonnet-20240620',
        'claude-3-opus-20240229',
        'claude-3-haiku-20240307',
      ]);
    });

    it('should not make HTTP request', async () => {
      await provider.models();
      expect(mockHttpClient.post).not.toHaveBeenCalled();
      expect(mockHttpClient.get).not.toHaveBeenCalled();
    });
  });

  describe('supportsStreaming', () => {
    it('should return true', () => {
      expect(provider.supportsStreaming()).toBe(true);
    });
  });

  describe('error message parsing', () => {
    it('should parse error message from error.message field', async () => {
      mockHttpClient.post.mockResolvedValue({
        status: 400,
        body: { error: { message: 'Custom error message' } },
      });

      try {
        await provider.completion({
          model: 'claude-3-opus-20240229',
          messages: [{ role: 'user', content: 'test' }],
        });
      } catch (error: any) {
        expect(error.message).toBe('Custom error message');
      }
    });

    it('should handle string error body', async () => {
      mockHttpClient.post.mockResolvedValue({
        status: 400,
        body: 'Plain text error',
      });

      try {
        await provider.completion({
          model: 'claude-3-opus-20240229',
          messages: [{ role: 'user', content: 'test' }],
        });
      } catch (error: any) {
        expect(error.message).toBe('Plain text error');
      }
    });

    it('should stringify unknown error format', async () => {
      const errorBody = { unknown: 'format' };
      mockHttpClient.post.mockResolvedValue({
        status: 400,
        body: errorBody,
      });

      try {
        await provider.completion({
          model: 'claude-3-opus-20240229',
          messages: [{ role: 'user', content: 'test' }],
        });
      } catch (error: any) {
        expect(error.message).toBe(JSON.stringify(errorBody));
      }
    });
  });
});
