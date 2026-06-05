/**
 * Cosmos LLM - A unified interface for interacting with multiple Large Language Model APIs.
 *
 * This library provides a consistent API across various LLM providers including OpenAI,
 * Anthropic, Google, Cohere, and more. It simplifies the integration of AI capabilities
 * into JavaScript/TypeScript applications.
 *
 * @example
 * ```typescript
 * import { CosmosLlm, configure } from 'cosmos-llm';
 *
 * // Configure API keys
 * configure((config) => {
 *   config.openai.apiKey = 'sk-...';
 *   config.anthropic.apiKey = 'sk-ant-...';
 * });
 *
 * // Create a client and make a request
 * const client = new CosmosLlm('openai', { model: 'gpt-4' });
 * const response = await client.complete('Hello, world!');
 * console.log(response);
 * ```
 */

import { Configuration } from './configuration';
import { Client, ClientOptions } from './client';

// Export all types and classes
export { Configuration } from './configuration';
export { Client } from './client';
export * from './errors';
export * from './providers';

/**
 * Global configuration instance.
 */
let globalConfig: Configuration | null = null;

/**
 * Gets or creates the global configuration instance.
 *
 * @returns The global configuration instance
 */
export function getConfiguration(): Configuration {
  if (!globalConfig) {
    globalConfig = new Configuration();
  }
  return globalConfig;
}

/**
 * Configures the global LLM settings.
 *
 * @param callback - Configuration callback function
 *
 * @example
 * ```typescript
 * configure((config) => {
 *   config.openai.apiKey = 'sk-...';
 *   config.anthropic.apiKey = 'sk-ant-...';
 *   config.defaultProvider = 'openai';
 * });
 * ```
 */
export function configure(callback: (config: Configuration) => void): void {
  const config = getConfiguration();
  callback(config);
}

/**
 * Main class for creating LLM clients.
 *
 * @example
 * ```typescript
 * // Create with provider and options
 * const client = new CosmosLlm('openai', { apiKey: 'sk-...', model: 'gpt-4' });
 *
 * // Simple completion
 * const response = await client.complete('What is the capital of France?');
 * console.log(response);
 *
 * // Chat with full control
 * const chatResponse = await client.chat({
 *   messages: [
 *     { role: 'system', content: 'You are a helpful assistant.' },
 *     { role: 'user', content: 'Hello!' }
 *   ],
 *   temperature: 0.7
 * });
 * ```
 */
export class CosmosLlm extends Client {
  constructor(provider?: string, options: ClientOptions = {}) {
    super(provider, options);
  }

  /**
   * Creates a new LLM client for the specified provider.
   *
   * @param provider - The provider name (e.g., 'openai', 'anthropic')
   * @param options - Configuration options for the client
   * @returns A new client instance
   *
   * @example
   * ```typescript
   * const client = CosmosLlm.create('openai', { apiKey: 'sk-...', model: 'gpt-4' });
   * ```
   */
  static create(provider?: string, options: ClientOptions = {}): CosmosLlm {
    return new CosmosLlm(provider, options);
  }

  /**
   * Quick completion with minimal setup.
   *
   * @param text - The input text to complete
   * @param options - Additional options including provider and model
   * @returns The completion text
   *
   * @example
   * ```typescript
   * const result = await CosmosLlm.quickComplete('What is Ruby?', {
   *   provider: 'openai',
   *   model: 'gpt-4'
   * });
   * console.log(result);
   * ```
   */
  static async quickComplete(
    text: string,
    options: { provider?: string; model?: string; apiKey?: string } = {}
  ): Promise<string> {
    const { provider = 'openai', model, apiKey, ...rest } = options;

    if (!model) {
      throw new Error('model is required');
    }

    const client = new CosmosLlm(provider, { model, apiKey, ...rest });
    return await client.complete(text);
  }

  /**
   * Quick chat with minimal setup.
   *
   * @param messages - Array of message objects
   * @param options - Additional options including provider and model
   * @returns The chat response
   *
   * @example
   * ```typescript
   * const response = await CosmosLlm.quickChat(
   *   [{ role: 'user', content: 'Hello!' }],
   *   { provider: 'openai', model: 'gpt-4' }
   * );
   * ```
   */
  static async quickChat(
    messages: Array<{ role: string; content: string }>,
    options: { provider?: string; model?: string; apiKey?: string; [key: string]: any } = {}
  ): Promise<any> {
    const { provider = 'openai', model, apiKey, ...rest } = options;

    if (!model) {
      throw new Error('model is required');
    }

    const client = new CosmosLlm(provider, { model, apiKey });
    return await client.chat({ messages, ...rest });
  }
}

/**
 * Default export for convenience.
 */
export default CosmosLlm;
