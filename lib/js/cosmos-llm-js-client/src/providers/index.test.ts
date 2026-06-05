/**
 * Unit tests for provider registry functions.
 */

import { describe, it, expect } from 'vitest';
import {
  getProviderClass,
  getAvailableProviders,
  isProviderAvailable,
  OpenAIProvider,
  AnthropicProvider,
} from './index';

describe('Provider Registry', () => {
  describe('getProviderClass', () => {
    it('should return OpenAIProvider for "openai"', () => {
      const ProviderClass = getProviderClass('openai');
      expect(ProviderClass).toBe(OpenAIProvider);
    });

    it('should return AnthropicProvider for "anthropic"', () => {
      const ProviderClass = getProviderClass('anthropic');
      expect(ProviderClass).toBe(AnthropicProvider);
    });

    it('should be case-insensitive', () => {
      expect(getProviderClass('OpenAI')).toBe(OpenAIProvider);
      expect(getProviderClass('OPENAI')).toBe(OpenAIProvider);
      expect(getProviderClass('Anthropic')).toBe(AnthropicProvider);
      expect(getProviderClass('ANTHROPIC')).toBe(AnthropicProvider);
    });

    it('should throw error for unknown provider', () => {
      expect(() => getProviderClass('unknown')).toThrow(
        "Provider 'unknown' not found"
      );
    });

    it('should list available providers in error message', () => {
      try {
        getProviderClass('invalid');
      } catch (error: any) {
        expect(error.message).toContain('openai');
        expect(error.message).toContain('anthropic');
      }
    });

    it('should include usage example in error message', () => {
      try {
        getProviderClass('invalid');
      } catch (error: any) {
        expect(error.message).toContain('CosmosLlm');
        expect(error.message).toContain('apiKey');
      }
    });

    it('should return a constructor that can be instantiated', () => {
      const ProviderClass = getProviderClass('openai');
      const instance = new ProviderClass({ apiKey: 'test' });
      expect(instance).toBeDefined();
      expect(instance).toBeInstanceOf(OpenAIProvider);
    });
  });

  describe('getAvailableProviders', () => {
    it('should return array of provider names', () => {
      const providers = getAvailableProviders();
      expect(Array.isArray(providers)).toBe(true);
    });

    it('should include openai provider', () => {
      const providers = getAvailableProviders();
      expect(providers).toContain('openai');
    });

    it('should include anthropic provider', () => {
      const providers = getAvailableProviders();
      expect(providers).toContain('anthropic');
    });

    it('should return at least 2 providers', () => {
      const providers = getAvailableProviders();
      expect(providers.length).toBeGreaterThanOrEqual(2);
    });

    it('should return lowercase provider names', () => {
      const providers = getAvailableProviders();
      providers.forEach((name) => {
        expect(name).toBe(name.toLowerCase());
      });
    });
  });

  describe('isProviderAvailable', () => {
    it('should return true for openai', () => {
      expect(isProviderAvailable('openai')).toBe(true);
    });

    it('should return true for anthropic', () => {
      expect(isProviderAvailable('anthropic')).toBe(true);
    });

    it('should return false for unknown provider', () => {
      expect(isProviderAvailable('unknown')).toBe(false);
      expect(isProviderAvailable('fake-provider')).toBe(false);
    });

    it('should be case-insensitive', () => {
      expect(isProviderAvailable('OpenAI')).toBe(true);
      expect(isProviderAvailable('OPENAI')).toBe(true);
      expect(isProviderAvailable('Anthropic')).toBe(true);
      expect(isProviderAvailable('ANTHROPIC')).toBe(true);
    });

    it('should handle empty string', () => {
      expect(isProviderAvailable('')).toBe(false);
    });

    it('should handle whitespace', () => {
      expect(isProviderAvailable('  ')).toBe(false);
    });
  });

  describe('Provider Registry Integration', () => {
    it('should have matching provider names between functions', () => {
      const availableProviders = getAvailableProviders();

      availableProviders.forEach((provider) => {
        expect(isProviderAvailable(provider)).toBe(true);
        expect(() => getProviderClass(provider)).not.toThrow();
      });
    });

    it('should instantiate all registered providers', () => {
      const providers = getAvailableProviders();

      providers.forEach((providerName) => {
        const ProviderClass = getProviderClass(providerName);
        const instance = new ProviderClass({ apiKey: 'test-key' });
        expect(instance).toBeDefined();
        expect(instance.completion).toBeDefined();
        expect(instance.models).toBeDefined();
      });
    });
  });
});
