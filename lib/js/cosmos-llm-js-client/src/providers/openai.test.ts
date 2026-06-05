/**
 * Unit tests for OpenAIProvider class.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { OpenAIProvider } from './openai';
import { HttpClient } from '../http-client';
import {
  AuthenticationError,
  RateLimitError,
  InvalidRequestError,
  ServerError,
  APIError,
} from '../errors';

vi.mock('../http-client', () => ({
  HttpClient: vi.fn(function(this: any) {
    return {
      post: vi.fn(),
      get: vi.fn(),
      postStream: vi.fn(),
    };
  }),
}));

describe('OpenAIProvider', () => {
  let provider: OpenAIProvider;
  let mockHttpClient: any;

  beforeEach(() => {
    vi.clearAllMocks();
    delete process.env.OPENAI_API_KEY;
    delete process.env.CLLM__OPENAI__API_KEY;
    delete process.env.OPENAI_ORGANIZATION;

    mockHttpClient = {
      post: vi.fn(),
      get: vi.fn(),
      postStream: vi.fn(),
    };

    vi.mocked(HttpClient).mockImplementation(function(this: any) {
      return mockHttpClient;
    });
    provider = new OpenAIProvider({ apiKey: 'test-key' });
  });

  describe('constructor', () => {
    it('should initialize with provided API key', () => {
      expect(provider).toBeDefined();
    });

    it('should use OPENAI_API_KEY from environment', () => {
      process.env.OPENAI_API_KEY = 'env-key';
      const envProvider = new OpenAIProvider();
      expect(envProvider).toBeDefined();
    });

    it('should use CLLM__OPENAI__API_KEY from environment', () => {
      process.env.CLLM__OPENAI__API_KEY = 'cllm-key';
      const envProvider = new OpenAIProvider();
      expect(envProvider).toBeDefined();
    });

    it('should set organization from options', () => {
      const orgProvider = new OpenAIProvider({
        apiKey: 'test-key',
        organization: 'org-123',
      });
      expect(orgProvider).toBeDefined();
    });

    it('should use organization from environment variable', () => {
      process.env.OPENAI_ORGANIZATION = 'org-env';
      const envOrgProvider = new OpenAIProvider({ apiKey: 'test-key' });
      expect(envOrgProvider).toBeDefined();
    });
  });

  describe('completion', () => {
    it('should make a POST request to chat/completions endpoint', async () => {
      mockHttpClient.post.mockResolvedValue({
        status: 200,
        body: { choices: [{ message: { content: 'Hello!' } }] },
      });

      const options = {
        model: 'gpt-4',
        messages: [{ role: 'user', content: 'Hi' }],
      };

      await provider.completion(options);

      expect(mockHttpClient.post).toHaveBeenCalledWith(
        'chat/completions',
        expect.objectContaining({
          model: 'gpt-4',
          messages: [{ role: 'user', content: 'Hi' }],
        }),
        expect.objectContaining({
          Authorization: 'Bearer test-key',
        })
      );
    });

    it('should include organization header when set', async () => {
      const orgProvider = new OpenAIProvider({
        apiKey: 'test-key',
        organization: 'org-123',
      });
      mockHttpClient.post.mockResolvedValue({
        status: 200,
        body: { choices: [] },
      });

      await orgProvider.completion({
        model: 'gpt-4',
        messages: [{ role: 'user', content: 'test' }],
      });

      expect(mockHttpClient.post).toHaveBeenCalledWith(
        expect.any(String),
        expect.any(Object),
        expect.objectContaining({
          'OpenAI-Organization': 'org-123',
        })
      );
    });

    it('should include optional parameters in request body', async () => {
      mockHttpClient.post.mockResolvedValue({
        status: 200,
        body: { choices: [] },
      });

      await provider.completion({
        model: 'gpt-4',
        messages: [{ role: 'user', content: 'test' }],
        temperature: 0.7,
        maxTokens: 100,
        topP: 0.9,
        frequencyPenalty: 0.5,
        presencePenalty: 0.5,
        stop: ['END'],
      });

      expect(mockHttpClient.post).toHaveBeenCalledWith(
        expect.any(String),
        expect.objectContaining({
          temperature: 0.7,
          max_tokens: 100,
          top_p: 0.9,
          frequency_penalty: 0.5,
          presence_penalty: 0.5,
          stop: ['END'],
        }),
        expect.any(Object)
      );
    });

    it('should throw AuthenticationError without API key', async () => {
      const noKeyProvider = new OpenAIProvider();
      await expect(
        noKeyProvider.completion({
          model: 'gpt-4',
          messages: [{ role: 'user', content: 'test' }],
        })
      ).rejects.toThrow(AuthenticationError);
    });

    it('should validate required parameters', async () => {
      await expect(
        provider.completion({
          model: 'gpt-4',
          messages: [],
        } as any)
      ).rejects.toThrow('Missing required parameters');
    });

    it('should return response body on success', async () => {
      const responseBody = { choices: [{ message: { content: 'Response' } }] };
      mockHttpClient.post.mockResolvedValue({
        status: 200,
        body: responseBody,
      });

      const result = await provider.completion({
        model: 'gpt-4',
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
          model: 'gpt-4',
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
          model: 'gpt-4',
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
          model: 'gpt-4',
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
          model: 'gpt-4',
          messages: [{ role: 'user', content: 'test' }],
        })
      ).rejects.toThrow(ServerError);
    });

    it('should throw APIError on unknown status codes', async () => {
      mockHttpClient.post.mockResolvedValue({
        status: 418,
        body: {},
      });

      await expect(
        provider.completion({
          model: 'gpt-4',
          messages: [{ role: 'user', content: 'test' }],
        })
      ).rejects.toThrow(APIError);
    });
  });

  describe('stream', () => {
    it('should make a streaming POST request', async () => {
      mockHttpClient.postStream.mockResolvedValue({
        status: 200,
        body: {},
      });

      const onChunk = vi.fn();
      await provider.stream({
        model: 'gpt-4',
        messages: [{ role: 'user', content: 'test' }],
        onChunk,
      });

      expect(mockHttpClient.postStream).toHaveBeenCalledWith(
        'chat/completions',
        expect.objectContaining({
          headers: expect.objectContaining({
            Authorization: 'Bearer test-key',
          }),
          body: expect.objectContaining({
            stream: true,
          }),
          onChunk,
        })
      );
    });

    it('should validate API key before streaming', async () => {
      const noKeyProvider = new OpenAIProvider();
      await expect(
        noKeyProvider.stream({
          model: 'gpt-4',
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
          model: 'gpt-4',
          messages: [{ role: 'user', content: 'test' }],
          onChunk: vi.fn(),
        })
      ).rejects.toThrow(AuthenticationError);
    });
  });

  describe('embedding', () => {
    it('should make a POST request to embeddings endpoint', async () => {
      mockHttpClient.post.mockResolvedValue({
        status: 200,
        body: { data: [{ embedding: [0.1, 0.2, 0.3] }] },
      });

      await provider.embedding({
        model: 'text-embedding-ada-002',
        input: 'test text',
      });

      expect(mockHttpClient.post).toHaveBeenCalledWith(
        'embeddings',
        expect.objectContaining({
          model: 'text-embedding-ada-002',
          input: 'test text',
        }),
        expect.objectContaining({
          Authorization: 'Bearer test-key',
        })
      );
    });

    it('should validate required parameters', async () => {
      await expect(
        provider.embedding({ model: 'test', input: '' } as any)
      ).rejects.toThrow('Missing required parameters');
    });

    it('should return embedding response', async () => {
      const responseBody = { data: [{ embedding: [0.1, 0.2] }] };
      mockHttpClient.post.mockResolvedValue({
        status: 200,
        body: responseBody,
      });

      const result = await provider.embedding({
        model: 'text-embedding-ada-002',
        input: 'test',
      });

      expect(result).toEqual(responseBody);
    });
  });

  describe('models', () => {
    it('should make a GET request to models endpoint', async () => {
      mockHttpClient.get.mockResolvedValue({
        status: 200,
        body: {
          data: [
            { id: 'gpt-4' },
            { id: 'gpt-3.5-turbo' },
            { id: 'text-embedding-ada-002' },
          ],
        },
      });

      await provider.models();

      expect(mockHttpClient.get).toHaveBeenCalledWith(
        'models',
        expect.objectContaining({
          Authorization: 'Bearer test-key',
        })
      );
    });

    it('should return array of model IDs', async () => {
      mockHttpClient.get.mockResolvedValue({
        status: 200,
        body: {
          data: [{ id: 'gpt-4' }, { id: 'gpt-3.5-turbo' }],
        },
      });

      const models = await provider.models();

      expect(models).toEqual(['gpt-4', 'gpt-3.5-turbo']);
    });

    it('should validate API key before fetching models', async () => {
      const noKeyProvider = new OpenAIProvider();
      await expect(noKeyProvider.models()).rejects.toThrow(AuthenticationError);
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
          model: 'gpt-4',
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
          model: 'gpt-4',
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
          model: 'gpt-4',
          messages: [{ role: 'user', content: 'test' }],
        });
      } catch (error: any) {
        expect(error.message).toBe(JSON.stringify(errorBody));
      }
    });
  });
});
