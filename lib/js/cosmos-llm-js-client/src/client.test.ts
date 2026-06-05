/**
 * Unit tests for Client class.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { Client } from './client';
import { BaseProvider } from './providers/base';

// Mock provider
class MockProvider extends BaseProvider {
  public completionCalled = false;
  public streamCalled = false;
  public embeddingCalled = false;

  protected getDefaultApiKey(): string | undefined {
    return 'mock-key';
  }

  async completion(options: any): Promise<any> {
    this.completionCalled = true;
    return {
      choices: [{ message: { content: 'Mock response' } }],
    };
  }

  async models(): Promise<string[]> {
    return ['mock-model'];
  }

  supportsStreaming(): boolean {
    return true;
  }

  async stream(options: any): Promise<void> {
    this.streamCalled = true;
    options.onChunk({ delta: 'chunk' });
  }

  async embedding(options: any): Promise<any> {
    this.embeddingCalled = true;
    return { data: [{ embedding: [0.1, 0.2] }] };
  }
}

// Mock getProviderClass
vi.mock('./providers', () => ({
  getProviderClass: vi.fn(() => MockProvider),
}));

describe('Client', () => {
  let client: Client;

  beforeEach(() => {
    vi.clearAllMocks();
    client = new Client('mock', { model: 'test-model' });
  });

  describe('constructor', () => {
    it('should initialize with provider and model', () => {
      expect(client).toBeDefined();
    });

    it('should extract model from options', () => {
      const clientWithModel = new Client('mock', { model: 'gpt-4' });
      expect(clientWithModel).toBeDefined();
    });

    it('should work without provider name', () => {
      const noProviderClient = new Client();
      expect(noProviderClient).toBeDefined();
    });
  });

  describe('complete', () => {
    it('should throw error for empty text', async () => {
      await expect(client.complete('')).rejects.toThrow(
        'Please provide text to complete'
      );
    });

    it('should throw error for whitespace-only text', async () => {
      await expect(client.complete('   ')).rejects.toThrow(
        'Please provide text to complete'
      );
    });

    it('should call provider completion with messages', async () => {
      const result = await client.complete('Hello');
      expect(result).toBe('Mock response');
    });

    it('should extract content from OpenAI-style response', async () => {
      const result = await client.complete('test');
      expect(result).toBe('Mock response');
    });

    it('should throw error when response has no content', async () => {
      const emptyClient = new Client('mock');
      const mockProvider = (emptyClient as any).provider;
      mockProvider.completion = vi.fn().mockResolvedValue({ choices: [] });

      await expect(emptyClient.complete('test')).rejects.toThrow(
        'did not return any content'
      );
    });

    it('should throw error when provider is not set', async () => {
      const noProviderClient = new Client();
      await expect(noProviderClient.complete('test')).rejects.toThrow(
        'No provider set'
      );
    });
  });

  describe('completion', () => {
    it('should throw error for non-object params', async () => {
      await expect(client.completion(null as any)).rejects.toThrow(
        'Parameters must be an object'
      );
    });

    it('should call provider completion with processed params', async () => {
      const result = await client.completion({
        messages: [{ role: 'user', content: 'test' }],
      });

      expect(result.choices).toBeDefined();
    });

    it('should include model from constructor', async () => {
      await client.completion({
        messages: [{ role: 'user', content: 'test' }],
      });

      const mockProvider = (client as any).provider;
      expect(mockProvider.completionCalled).toBe(true);
    });
  });

  describe('chat', () => {
    it('should be an alias for completion', async () => {
      const completionSpy = vi.spyOn(client, 'completion');
      await client.chat({ messages: [{ role: 'user', content: 'test' }] });

      expect(completionSpy).toHaveBeenCalled();
    });
  });

  describe('embed', () => {
    it('should throw error for non-object params', async () => {
      await expect(client.embed(null as any)).rejects.toThrow(
        'Parameters must be an object'
      );
    });

    it('should call provider embedding', async () => {
      const result = await client.embed({
        model: 'text-embedding-ada-002',
        input: 'test',
      });

      expect(result.data).toBeDefined();
    });

    it('should provide helpful error when provider does not support embeddings', async () => {
      const mockProvider = (client as any).provider;
      mockProvider.embedding = vi
        .fn()
        .mockRejectedValue(new Error('MockProvider does not support embeddings'));

      await expect(
        client.embed({ model: 'test', input: 'text' })
      ).rejects.toThrow('does not support embeddings');
    });
  });

  describe('stream', () => {
    it('should throw error for non-object params', async () => {
      await expect(client.stream(null as any, vi.fn())).rejects.toThrow(
        'Parameters must be an object'
      );
    });

    it('should throw error for missing callback', async () => {
      await expect(
        client.stream({ messages: [] }, null as any)
      ).rejects.toThrow('requires a callback function');
    });

    it('should call provider stream with callback', async () => {
      const callback = vi.fn();
      await client.stream(
        { messages: [{ role: 'user', content: 'test' }] },
        callback
      );

      const mockProvider = (client as any).provider;
      expect(mockProvider.streamCalled).toBe(true);
    });

    it('should provide helpful error when provider does not support streaming', async () => {
      const mockProvider = (client as any).provider;
      mockProvider.supportsStreaming = () => false;
      mockProvider.stream = vi
        .fn()
        .mockRejectedValue(new Error('MockProvider does not support streaming'));

      await expect(
        client.stream({ messages: [] }, vi.fn())
      ).rejects.toThrow('does not support streaming');
    });
  });

  describe('canStream', () => {
    it('should return true when provider supports streaming', () => {
      expect(client.canStream()).toBe(true);
    });

    it('should return false when provider does not support streaming', () => {
      const mockProvider = (client as any).provider;
      mockProvider.supportsStreaming = () => false;
      expect(client.canStream()).toBe(false);
    });

    it('should return false when provider is not set', () => {
      const noProviderClient = new Client();
      expect(noProviderClient.canStream()).toBe(false);
    });
  });

  describe('fluent interface', () => {
    it('withProvider should set provider', () => {
      const result = client.withProvider('mock');
      expect(result).toBe(client);
    });

    it('withProvider should throw error for empty name', () => {
      expect(() => client.withProvider('')).toThrow('Please specify a provider name');
    });

    it('withModel should set model', () => {
      const result = client.withModel('gpt-4');
      expect(result).toBe(client);
    });

    it('withTemperature should set temperature for next request', async () => {
      await client.withTemperature(0.5).complete('test');
      // Temperature should be cleared after use
      expect((client as any).nextTemperature).toBeUndefined();
    });

    it('withMaxTokens should set max tokens for next request', async () => {
      await client.withMaxTokens(100).complete('test');
      expect((client as any).nextMaxTokens).toBeUndefined();
    });

    it('withTopP should set top_p for next request', async () => {
      await client.withTopP(0.9).complete('test');
      expect((client as any).nextTopP).toBeUndefined();
    });

    it('withStop should set stop sequences for next request', async () => {
      await client.withStop(['END']).complete('test');
      expect((client as any).nextStop).toBeUndefined();
    });

    it('withSystem should prepend system message', async () => {
      await client.withSystem('You are helpful').complete('test');
      expect((client as any).nextSystem).toBeUndefined();
    });

    it('should allow chaining multiple fluent methods', async () => {
      const result = await client
        .withTemperature(0.7)
        .withMaxTokens(100)
        .withTopP(0.9)
        .complete('test');

      expect(result).toBeDefined();
    });
  });

  describe('cloneWith', () => {
    it('should create a new client with merged options', () => {
      const clone = client.cloneWith({ model: 'new-model' });
      expect(clone).not.toBe(client);
      expect(clone).toBeInstanceOf(Client);
    });

    it('should preserve provider from original client', () => {
      const clone = client.cloneWith({ model: 'new-model' });
      expect(clone).toBeDefined();
    });
  });

  describe('processParams', () => {
    it('should merge default model into params', async () => {
      await client.complete('test');
      // Verify model was included in request
      const mockProvider = (client as any).provider;
      expect(mockProvider.completionCalled).toBe(true);
    });

    it('should apply fluent settings to params', async () => {
      client.withTemperature(0.8);
      await client.complete('test');
      // Verify fluent settings are cleared
      expect((client as any).nextTemperature).toBeUndefined();
    });

    it('should prepend system message when set', async () => {
      client.withSystem('System prompt');
      await client.complete('User message');
      expect((client as any).nextSystem).toBeUndefined();
    });
  });

  describe('extractContent', () => {
    it('should extract content from OpenAI-style response', async () => {
      const mockProvider = (client as any).provider;
      mockProvider.completion = vi.fn().mockResolvedValue({
        choices: [{ message: { content: 'OpenAI response' } }],
      });

      const result = await client.complete('test');
      expect(result).toBe('OpenAI response');
    });

    it('should extract content from Anthropic-style response', async () => {
      const mockProvider = (client as any).provider;
      mockProvider.completion = vi.fn().mockResolvedValue({
        content: [
          { type: 'text', text: 'Anthropic ' },
          { type: 'text', text: 'response' },
        ],
      });

      const result = await client.complete('test');
      expect(result).toBe('Anthropic  response');
    });

    it('should extract direct content field', async () => {
      const mockProvider = (client as any).provider;
      mockProvider.completion = vi.fn().mockResolvedValue({
        content: 'Direct content',
      });

      const result = await client.complete('test');
      expect(result).toBe('Direct content');
    });

    it('should return null for empty responses', async () => {
      const mockProvider = (client as any).provider;
      mockProvider.completion = vi.fn().mockResolvedValue({});

      await expect(client.complete('test')).rejects.toThrow(
        'did not return any content'
      );
    });
  });

  describe('error handling', () => {
    it('should throw error when provider is not initialized', async () => {
      const uninitializedClient = new Client();
      await expect(uninitializedClient.complete('test')).rejects.toThrow(
        'No provider set'
      );
    });

    it('should include helpful message in provider error', async () => {
      const uninitializedClient = new Client();
      try {
        await uninitializedClient.complete('test');
      } catch (error: any) {
        expect(error.message).toContain('withProvider');
        expect(error.message).toContain('openai');
      }
    });
  });
});
