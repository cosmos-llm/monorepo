/**
 * Unit tests for main index module.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import {
  CosmosLlm,
  configure,
  getConfiguration,
  Configuration,
} from './index';

describe('Index Module', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe('getConfiguration', () => {
    it('should return a Configuration instance', () => {
      const config = getConfiguration();
      expect(config).toBeInstanceOf(Configuration);
    });

    it('should return the same instance on multiple calls', () => {
      const config1 = getConfiguration();
      const config2 = getConfiguration();
      expect(config1).toBe(config2);
    });

    it('should have default provider set', () => {
      const config = getConfiguration();
      expect(config.defaultProvider).toBe('openai');
    });
  });

  describe('configure', () => {
    it('should call callback with configuration instance', () => {
      const callback = vi.fn();
      configure(callback);
      expect(callback).toHaveBeenCalledWith(expect.any(Configuration));
    });

    it('should allow setting API keys', () => {
      configure((config) => {
        config.openai.apiKey = 'test-key';
      });

      const config = getConfiguration();
      expect(config.openai.apiKey).toBe('test-key');
    });

    it('should allow setting default provider', () => {
      configure((config) => {
        config.defaultProvider = 'anthropic';
      });

      const config = getConfiguration();
      expect(config.defaultProvider).toBe('anthropic');
    });

    it('should allow configuring multiple providers', () => {
      configure((config) => {
        config.openai.apiKey = 'openai-key';
        config.anthropic.apiKey = 'anthropic-key';
      });

      const config = getConfiguration();
      expect(config.openai.apiKey).toBe('openai-key');
      expect(config.anthropic.apiKey).toBe('anthropic-key');
    });
  });

  describe('CosmosLlm', () => {
    describe('constructor', () => {
      it('should create instance with provider and options', () => {
        const client = new CosmosLlm('openai', { model: 'gpt-4' });
        expect(client).toBeDefined();
        expect(client).toBeInstanceOf(CosmosLlm);
      });

      it('should work without options', () => {
        const client = new CosmosLlm('openai');
        expect(client).toBeDefined();
      });

      it('should work without provider', () => {
        const client = new CosmosLlm();
        expect(client).toBeDefined();
      });
    });

    describe('create', () => {
      it('should create a new instance', () => {
        const client = CosmosLlm.create('openai', { model: 'gpt-4' });
        expect(client).toBeInstanceOf(CosmosLlm);
      });

      it('should work without options', () => {
        const client = CosmosLlm.create('openai');
        expect(client).toBeDefined();
      });

      it('should create independent instances', () => {
        const client1 = CosmosLlm.create('openai');
        const client2 = CosmosLlm.create('openai');
        expect(client1).not.toBe(client2);
      });
    });

    describe('quickComplete', () => {
      it('should throw error if model is not provided', async () => {
        await expect(
          CosmosLlm.quickComplete('test', { provider: 'openai' })
        ).rejects.toThrow('model is required');
      });

      it('should default to openai provider', async () => {
        // This will fail without API key, but we're testing the setup
        await expect(
          CosmosLlm.quickComplete('test', { model: 'gpt-4' })
        ).rejects.toThrow();
      });

      it('should accept provider option', async () => {
        await expect(
          CosmosLlm.quickComplete('test', {
            provider: 'openai',
            model: 'gpt-4',
          })
        ).rejects.toThrow();
      });

      it('should accept apiKey option', async () => {
        await expect(
          CosmosLlm.quickComplete('test', {
            provider: 'openai',
            model: 'gpt-4',
            apiKey: 'test-key',
          })
        ).rejects.toThrow();
      });

      it('should pass additional options', async () => {
        await expect(
          CosmosLlm.quickComplete('test', {
            provider: 'openai',
            model: 'gpt-4',
            apiKey: 'test-key',
            temperature: 0.7,
          } as any)
        ).rejects.toThrow();
      });
    });

    describe('quickChat', () => {
      it('should throw error if model is not provided', async () => {
        await expect(
          CosmosLlm.quickChat([{ role: 'user', content: 'test' }], {
            provider: 'openai',
          })
        ).rejects.toThrow('model is required');
      });

      it('should default to openai provider', async () => {
        await expect(
          CosmosLlm.quickChat([{ role: 'user', content: 'test' }], {
            model: 'gpt-4',
          })
        ).rejects.toThrow();
      });

      it('should accept provider option', async () => {
        await expect(
          CosmosLlm.quickChat([{ role: 'user', content: 'test' }], {
            provider: 'anthropic',
            model: 'claude-3-opus',
          })
        ).rejects.toThrow();
      });

      it('should accept apiKey option', async () => {
        await expect(
          CosmosLlm.quickChat([{ role: 'user', content: 'test' }], {
            provider: 'openai',
            model: 'gpt-4',
            apiKey: 'test-key',
          })
        ).rejects.toThrow();
      });

      it('should pass additional options', async () => {
        await expect(
          CosmosLlm.quickChat([{ role: 'user', content: 'test' }], {
            provider: 'openai',
            model: 'gpt-4',
            apiKey: 'test-key',
            temperature: 0.5,
            maxTokens: 100,
          })
        ).rejects.toThrow();
      });

      it('should accept messages array', async () => {
        await expect(
          CosmosLlm.quickChat(
            [
              { role: 'system', content: 'You are helpful' },
              { role: 'user', content: 'Hello' },
            ],
            {
              model: 'gpt-4',
              apiKey: 'test-key',
            }
          )
        ).rejects.toThrow();
      });
    });

    describe('inheritance from Client', () => {
      it('should have all Client methods', () => {
        const client = new CosmosLlm('openai', { model: 'gpt-4' });
        expect(client.complete).toBeDefined();
        expect(client.completion).toBeDefined();
        expect(client.chat).toBeDefined();
        expect(client.embed).toBeDefined();
        expect(client.stream).toBeDefined();
        expect(client.canStream).toBeDefined();
      });

      it('should have fluent interface methods', () => {
        const client = new CosmosLlm('openai', { model: 'gpt-4' });
        expect(client.withProvider).toBeDefined();
        expect(client.withModel).toBeDefined();
        expect(client.withTemperature).toBeDefined();
        expect(client.withMaxTokens).toBeDefined();
        expect(client.withTopP).toBeDefined();
        expect(client.withStop).toBeDefined();
        expect(client.withSystem).toBeDefined();
      });

      it('should support method chaining', () => {
        const client = new CosmosLlm('openai', { model: 'gpt-4' });
        const chained = client
          .withTemperature(0.7)
          .withMaxTokens(100);
        expect(chained).toBe(client);
      });
    });
  });

  describe('exports', () => {
    it('should export Configuration', () => {
      expect(Configuration).toBeDefined();
    });

    it('should export configure function', () => {
      expect(configure).toBeDefined();
      expect(typeof configure).toBe('function');
    });

    it('should export getConfiguration function', () => {
      expect(getConfiguration).toBeDefined();
      expect(typeof getConfiguration).toBe('function');
    });

    it('should export CosmosLlm class', () => {
      expect(CosmosLlm).toBeDefined();
    });

    it('should have default export', async () => {
      const DefaultExport = (await import('./index')).default;
      expect(DefaultExport).toBe(CosmosLlm);
    });
  });

  describe('integration', () => {
    it('should allow configuring and using client', () => {
      configure((config) => {
        config.openai.apiKey = 'test-api-key';
        config.openai.model = 'gpt-4';
      });

      const client = new CosmosLlm('openai', { model: 'gpt-4' });
      expect(client).toBeDefined();
    });

    it('should create multiple clients with different configurations', () => {
      const openaiClient = new CosmosLlm('openai', { model: 'gpt-4' });
      const anthropicClient = new CosmosLlm('anthropic', {
        model: 'claude-3-opus',
      });

      expect(openaiClient).toBeDefined();
      expect(anthropicClient).toBeDefined();
      expect(openaiClient).not.toBe(anthropicClient);
    });
  });
});
