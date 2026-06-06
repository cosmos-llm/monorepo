/**
 * Provider registry and exports.
 *
 * This module provides a centralized registry for all LLM providers
 * and helper functions for provider lookup.
 */

import { BaseProvider } from './base';
import { OpenAIProvider } from './openai';
import { AnthropicProvider } from './anthropic';

export { BaseProvider } from './base';
export { OpenAIProvider } from './openai';
export { AnthropicProvider } from './anthropic';

/**
 * Type for provider constructor options.
 */
export type ProviderOptions = any;

/**
 * Type for provider constructors.
 */
export type ProviderConstructor = new (options?: ProviderOptions) => BaseProvider;

/**
 * Registry of available providers.
 */
const PROVIDER_REGISTRY: Map<string, ProviderConstructor> = new Map<string, ProviderConstructor>([
  ['openai', OpenAIProvider],
  ['anthropic', AnthropicProvider],
]);

/**
 * Gets the provider class for the given provider name.
 *
 * @param providerName - The name of the provider (case-insensitive)
 * @returns The provider constructor
 * @throws Error if provider is not found
 */
export function getProviderClass(providerName: string): ProviderConstructor {
  const name = providerName.toLowerCase();
  const ProviderClass = PROVIDER_REGISTRY.get(name);

  if (!ProviderClass) {
    const available = Array.from(PROVIDER_REGISTRY.keys()).join(', ');
    throw new Error(
      `Provider '${providerName}' not found.\n\n` +
        `Available providers: ${available}\n\n` +
        `Example: new CosmosLlm('openai', { apiKey: 'sk-...' })`
    );
  }

  return ProviderClass;
}

/**
 * Gets the list of available provider names.
 *
 * @returns Array of provider names
 */
export function getAvailableProviders(): string[] {
  return Array.from(PROVIDER_REGISTRY.keys());
}

/**
 * Checks if a provider is available.
 *
 * @param providerName - The name of the provider
 * @returns True if the provider exists, false otherwise
 */
export function isProviderAvailable(providerName: string): boolean {
  return PROVIDER_REGISTRY.has(providerName.toLowerCase());
}
