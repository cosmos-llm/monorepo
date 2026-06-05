/**
 * Unit tests for BaseProvider abstract class.
 */

import { describe, it, expect, beforeEach } from 'vitest';
import { BaseProvider, CompletionOptions, EmbeddingOptions } from './base';
import { AuthenticationError } from '../errors';

// Create a concrete implementation for testing
class TestProvider extends BaseProvider {
  constructor(apiKey?: string) {
    super(apiKey);
  }

  protected getDefaultApiKey(): string | undefined {
    return process.env.TEST_API_KEY;
  }

  async completion(options: CompletionOptions): Promise<any> {
    this.validateApiKey();
    this.validateRequiredParams(options, ['model', 'messages']);
    return { success: true, model: options.model };
  }

  async models(): Promise<string[]> {
    return ['model-1', 'model-2', 'model-3'];
  }
}

// Provider with streaming support
class StreamingTestProvider extends BaseProvider {
  protected getDefaultApiKey(): string | undefined {
    return process.env.TEST_API_KEY;
  }

  async completion(options: CompletionOptions): Promise<any> {
    return { success: true };
  }

  async models(): Promise<string[]> {
    return ['streaming-model'];
  }

  supportsStreaming(): boolean {
    return true;
  }

  async stream(options: any): Promise<void> {
    options.onChunk({ delta: 'test' });
  }
}

describe('BaseProvider', () => {
  let provider: TestProvider;

  beforeEach(() => {
    delete process.env.TEST_API_KEY;
    provider = new TestProvider();
  });

  describe('constructor', () => {
    it('should initialize with provided API key', () => {
      const providerWithKey = new TestProvider('test-api-key');
      expect(providerWithKey).toBeDefined();
    });

    it('should use default API key from environment if not provided', () => {
      process.env.TEST_API_KEY = 'env-key';
      const providerWithEnvKey = new TestProvider();
      expect(providerWithEnvKey).toBeDefined();
    });
  });

  describe('validateApiKey', () => {
    it('should throw AuthenticationError if API key is not set', () => {
      expect(() => {
        (provider as any).validateApiKey();
      }).toThrow(AuthenticationError);
    });

    it('should throw AuthenticationError if API key is empty string', () => {
      const emptyKeyProvider = new TestProvider('   ');
      expect(() => {
        (emptyKeyProvider as any).validateApiKey();
      }).toThrow(AuthenticationError);
    });

    it('should not throw if API key is set', () => {
      const validProvider = new TestProvider('valid-key');
      expect(() => {
        (validProvider as any).validateApiKey();
      }).not.toThrow();
    });

    it('should include helpful error message with environment variable name', () => {
      try {
        (provider as any).validateApiKey();
      } catch (error: any) {
        expect(error.message).toContain('CLLM__TESTPROVIDER__API_KEY');
      }
    });
  });

  describe('validateRequiredParams', () => {
    it('should not throw when all required params are present', () => {
      const options = { model: 'gpt-4', messages: [{ role: 'user', content: 'test' }] };
      expect(() => {
        (provider as any).validateRequiredParams(options, ['model', 'messages']);
      }).not.toThrow();
    });

    it('should throw when required param is undefined', () => {
      const options = { messages: [{ role: 'user', content: 'test' }] };
      expect(() => {
        (provider as any).validateRequiredParams(options, ['model', 'messages']);
      }).toThrow('Missing required parameters: model');
    });

    it('should throw when required param is null', () => {
      const options = { model: null, messages: [] };
      expect(() => {
        (provider as any).validateRequiredParams(options, ['model', 'messages']);
      }).toThrow('Missing required parameters');
    });

    it('should throw when required param is empty string', () => {
      const options = { model: '   ', messages: [{ role: 'user', content: 'test' }] };
      expect(() => {
        (provider as any).validateRequiredParams(options, ['model']);
      }).toThrow('Missing required parameters: model');
    });

    it('should throw when required param is empty array', () => {
      const options = { model: 'gpt-4', messages: [] };
      expect(() => {
        (provider as any).validateRequiredParams(options, ['model', 'messages']);
      }).toThrow('Missing required parameters: messages');
    });

    it('should list all missing parameters', () => {
      const options = {};
      try {
        (provider as any).validateRequiredParams(options, ['model', 'messages', 'temperature']);
      } catch (error: any) {
        expect(error.message).toContain('model');
        expect(error.message).toContain('messages');
        expect(error.message).toContain('temperature');
      }
    });
  });

  describe('validateRange', () => {
    it('should not throw when value is within range', () => {
      expect(() => {
        (provider as any).validateRange(0.5, 'temperature', 0, 1);
      }).not.toThrow();
    });

    it('should not throw when value is at minimum boundary', () => {
      expect(() => {
        (provider as any).validateRange(0, 'temperature', 0, 1);
      }).not.toThrow();
    });

    it('should not throw when value is at maximum boundary', () => {
      expect(() => {
        (provider as any).validateRange(1, 'temperature', 0, 1);
      }).not.toThrow();
    });

    it('should throw when value is below minimum', () => {
      expect(() => {
        (provider as any).validateRange(-0.1, 'temperature', 0, 1);
      }).toThrow('outside the allowed range');
    });

    it('should throw when value is above maximum', () => {
      expect(() => {
        (provider as any).validateRange(1.1, 'temperature', 0, 1);
      }).toThrow('outside the allowed range');
    });

    it('should not throw when value is undefined', () => {
      expect(() => {
        (provider as any).validateRange(undefined, 'temperature', 0, 1);
      }).not.toThrow();
    });

    it('should not throw when value is null', () => {
      expect(() => {
        (provider as any).validateRange(null, 'temperature', 0, 1);
      }).not.toThrow();
    });

    it('should include parameter name and range in error message', () => {
      try {
        (provider as any).validateRange(5, 'temperature', 0, 2);
      } catch (error: any) {
        expect(error.message).toContain('temperature');
        expect(error.message).toContain('0');
        expect(error.message).toContain('2');
      }
    });
  });

  describe('supportsStreaming', () => {
    it('should return false by default', () => {
      expect(provider.supportsStreaming()).toBe(false);
    });

    it('should return true when overridden', () => {
      const streamingProvider = new StreamingTestProvider();
      expect(streamingProvider.supportsStreaming()).toBe(true);
    });
  });

  describe('stream', () => {
    it('should throw error by default', async () => {
      await expect(provider.stream({} as any)).rejects.toThrow(
        'does not support streaming'
      );
    });

    it('should work when overridden in subclass', async () => {
      const streamingProvider = new StreamingTestProvider();
      const chunks: any[] = [];
      await streamingProvider.stream({
        onChunk: (chunk: any) => chunks.push(chunk),
      } as any);

      expect(chunks).toHaveLength(1);
      expect(chunks[0]).toEqual({ delta: 'test' });
    });
  });

  describe('embedding', () => {
    it('should throw error by default', async () => {
      await expect(
        provider.embedding({ model: 'test', input: 'text' })
      ).rejects.toThrow('does not support embeddings');
    });

    it('should include helpful message in error', async () => {
      try {
        await provider.embedding({ model: 'test', input: 'text' });
      } catch (error: any) {
        expect(error.message).toContain('does not support embeddings');
      }
    });
  });

  describe('completion', () => {
    it('should validate API key before making request', async () => {
      await expect(
        provider.completion({
          model: 'test',
          messages: [{ role: 'user', content: 'test' }],
        })
      ).rejects.toThrow(AuthenticationError);
    });

    it('should validate required parameters', async () => {
      const validProvider = new TestProvider('valid-key');
      await expect(
        validProvider.completion({ model: 'test', messages: [] } as any)
      ).rejects.toThrow('Missing required parameters: messages');
    });

    it('should complete successfully with valid params', async () => {
      const validProvider = new TestProvider('valid-key');
      const result = await validProvider.completion({
        model: 'test-model',
        messages: [{ role: 'user', content: 'hello' }],
      });

      expect(result.success).toBe(true);
      expect(result.model).toBe('test-model');
    });
  });

  describe('models', () => {
    it('should return array of available models', async () => {
      const models = await provider.models();
      expect(Array.isArray(models)).toBe(true);
      expect(models).toHaveLength(3);
      expect(models).toContain('model-1');
      expect(models).toContain('model-2');
      expect(models).toContain('model-3');
    });
  });
});
