/**
 * Unit tests for Configuration class.
 */

import { describe, it, expect, beforeEach } from 'vitest';
import { Configuration } from './configuration';

describe('Configuration', () => {
  let config: Configuration;

  beforeEach(() => {
    // Clear environment variables before each test
    delete process.env.CLLM__OPENAI__API_KEY;
    delete process.env.CLLM__ANTHROPIC__API_KEY;
    delete process.env.CLLM__OPENAI__MODEL;
    delete process.env.CLLM__OPENAI__MAX_TOKENS;
    config = new Configuration();
  });

  describe('constructor', () => {
    it('should initialize with default provider as openai', () => {
      expect(config.defaultProvider).toBe('openai');
    });

    it('should load configuration from environment variables', () => {
      process.env.CLLM__OPENAI__API_KEY = 'test-key';
      process.env.CLLM__ANTHROPIC__MODEL = 'claude-3-opus';

      const newConfig = new Configuration();

      expect(newConfig.openai.apiKey).toBe('test-key');
      expect(newConfig.anthropic.model).toBe('claude-3-opus');
    });

    it('should convert environment variable names from SCREAMING_SNAKE_CASE to camelCase', () => {
      process.env.CLLM__OPENAI__API_KEY = 'sk-test';
      process.env.CLLM__OPENAI__MAX_TOKENS = '1000';

      const newConfig = new Configuration();

      expect(newConfig.openai.apiKey).toBe('sk-test');
      expect(newConfig.openai.maxTokens).toBe('1000');
    });
  });

  describe('getProvider', () => {
    it('should return empty config for new provider', () => {
      const providerConfig = config.getProvider('openai');
      expect(providerConfig).toEqual({});
    });

    it('should return existing config for configured provider', () => {
      config.openai.apiKey = 'test-key';
      const providerConfig = config.getProvider('openai');
      expect(providerConfig.apiKey).toBe('test-key');
    });

    it('should create provider config if it does not exist', () => {
      const providerConfig = config.getProvider('newprovider');
      expect(providerConfig).toBeDefined();
      expect(providerConfig).toEqual({});
    });
  });

  describe('setProvider', () => {
    it('should set configuration for a provider', () => {
      config.setProvider('openai', { apiKey: 'sk-123', model: 'gpt-4' });

      expect(config.openai.apiKey).toBe('sk-123');
      expect(config.openai.model).toBe('gpt-4');
    });

    it('should merge with existing configuration', () => {
      config.setProvider('openai', { apiKey: 'sk-123' });
      config.setProvider('openai', { model: 'gpt-4' });

      expect(config.openai.apiKey).toBe('sk-123');
      expect(config.openai.model).toBe('gpt-4');
    });

    it('should overwrite existing values when setting same key', () => {
      config.setProvider('openai', { apiKey: 'sk-old' });
      config.setProvider('openai', { apiKey: 'sk-new' });

      expect(config.openai.apiKey).toBe('sk-new');
    });
  });

  describe('clear', () => {
    it('should clear all provider configurations', () => {
      config.openai.apiKey = 'sk-123';
      config.anthropic.apiKey = 'sk-ant-123';
      config.defaultProvider = 'anthropic';

      config.clear();

      expect(config.openai.apiKey).toBeUndefined();
      expect(config.anthropic.apiKey).toBeUndefined();
    });

    it('should reset default provider to openai', () => {
      config.defaultProvider = 'anthropic';
      config.clear();

      expect(config.defaultProvider).toBe('openai');
    });

    it('should reload from environment variables after clear', () => {
      process.env.CLLM__OPENAI__API_KEY = 'env-key';
      config.clear();

      expect(config.openai.apiKey).toBe('env-key');
    });
  });

  describe('provider getters and setters', () => {
    it('should get and set openai configuration', () => {
      config.openai = { apiKey: 'sk-123', model: 'gpt-4' };

      expect(config.openai.apiKey).toBe('sk-123');
      expect(config.openai.model).toBe('gpt-4');
    });

    it('should get and set anthropic configuration', () => {
      config.anthropic = { apiKey: 'sk-ant-123', model: 'claude-3-opus' };

      expect(config.anthropic.apiKey).toBe('sk-ant-123');
      expect(config.anthropic.model).toBe('claude-3-opus');
    });

    it('should get and set google configuration', () => {
      config.google = { apiKey: 'google-key' };
      expect(config.google.apiKey).toBe('google-key');
    });

    it('should get and set cohere configuration', () => {
      config.cohere = { apiKey: 'cohere-key' };
      expect(config.cohere.apiKey).toBe('cohere-key');
    });

    it('should get and set mistral configuration', () => {
      config.mistral = { apiKey: 'mistral-key' };
      expect(config.mistral.apiKey).toBe('mistral-key');
    });

    it('should get and set groq configuration', () => {
      config.groq = { apiKey: 'groq-key' };
      expect(config.groq.apiKey).toBe('groq-key');
    });

    it('should get and set fireworks configuration', () => {
      config.fireworks = { apiKey: 'fireworks-key' };
      expect(config.fireworks.apiKey).toBe('fireworks-key');
    });

    it('should get and set together configuration', () => {
      config.together = { apiKey: 'together-key' };
      expect(config.together.apiKey).toBe('together-key');
    });

    it('should get and set deepseek configuration', () => {
      config.deepseek = { apiKey: 'deepseek-key' };
      expect(config.deepseek.apiKey).toBe('deepseek-key');
    });

    it('should get and set openrouter configuration', () => {
      config.openrouter = { apiKey: 'openrouter-key' };
      expect(config.openrouter.apiKey).toBe('openrouter-key');
    });

    it('should get and set perplexity configuration', () => {
      config.perplexity = { apiKey: 'perplexity-key' };
      expect(config.perplexity.apiKey).toBe('perplexity-key');
    });

    it('should get and set xai configuration', () => {
      config.xai = { apiKey: 'xai-key' };
      expect(config.xai.apiKey).toBe('xai-key');
    });

    it('should get and set azureopenai configuration', () => {
      config.azureopenai = { apiKey: 'azure-key' };
      expect(config.azureopenai.apiKey).toBe('azure-key');
    });

    it('should get and set huggingface configuration', () => {
      config.huggingface = { apiKey: 'hf-key' };
      expect(config.huggingface.apiKey).toBe('hf-key');
    });

    it('should get and set opencode configuration', () => {
      config.opencode = { apiKey: 'opencode-key' };
      expect(config.opencode.apiKey).toBe('opencode-key');
    });
  });

  describe('environment variable loading', () => {
    it('should ignore environment variables without CLLM__ prefix', () => {
      process.env.RANDOM_VAR = 'value';
      const newConfig = new Configuration();

      expect(newConfig.getProvider('random')).toEqual({});
    });

    it('should ignore malformed environment variable names', () => {
      process.env.CLLM__INCOMPLETE = 'value';
      const newConfig = new Configuration();

      expect(newConfig.getProvider('incomplete')).toEqual({});
    });

    it('should handle multiple underscores in setting names', () => {
      process.env.CLLM__OPENAI__MAX_TOKEN_LIMIT = '2000';
      const newConfig = new Configuration();

      expect(newConfig.openai.maxTokenLimit).toBe('2000');
    });
  });

  describe('defaultProvider', () => {
    it('should allow changing default provider', () => {
      config.defaultProvider = 'anthropic';
      expect(config.defaultProvider).toBe('anthropic');
    });

    it('should accept any string as default provider', () => {
      config.defaultProvider = 'custom-provider';
      expect(config.defaultProvider).toBe('custom-provider');
    });
  });
});
