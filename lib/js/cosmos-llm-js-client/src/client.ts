/**
 * Unified client interface for interacting with different LLM providers.
 *
 * The Client class provides a facade that delegates operations like completion, chat,
 * embedding, and streaming to the appropriate provider instance while handling parameter
 * processing, model configuration, and providing convenience methods.
 */

import { BaseProvider, CompletionOptions } from './providers/base';
import { getProviderClass } from './providers';

export interface ClientOptions {
  apiKey?: string;
  model?: string;
  [key: string]: any;
}

export interface SimpleCompletionOptions {
  [key: string]: any;
}

/**
 * Main client class for interacting with LLM providers.
 */
export class Client {
  private provider?: BaseProvider;
  private defaultModel?: string;
  private providerOptions: ClientOptions;

  // Fluent interface temporary settings
  private nextTemperature?: number;
  private nextMaxTokens?: number;
  private nextTopP?: number;
  private nextStop?: string | string[];
  private nextSystem?: string;

  constructor(providerName?: string, options: ClientOptions = {}) {
    if (options.model) {
      this.defaultModel = options.model;
      delete options.model;
    }

    this.providerOptions = options;

    if (providerName) {
      this.initializeProvider(providerName, options);
    }
  }

  /**
   * Initializes the provider.
   *
   * @param providerName - The provider name
   * @param options - Provider options
   */
  private initializeProvider(providerName: string, options: ClientOptions): void {
    const ProviderClass = getProviderClass(providerName);
    this.provider = new ProviderClass(options);
  }

  /**
   * Performs a simple text completion with minimal configuration.
   *
   * @param text - The input text to complete
   * @param opts - Additional options (reserved for future use)
   * @returns The generated completion text
   */
  async complete(text: string, _opts: SimpleCompletionOptions = {}): Promise<string> {
    if (!text || text.trim() === '') {
      throw new Error(
        'Please provide text to complete.\n' +
          "Example: client.complete('What is the capital of France?')"
      );
    }

    this.ensureProvider();

    const params = this.processParams({
      messages: [{ role: 'user', content: text }],
    });

    const response = await this.provider!.completion(params);

    // Extract content from response
    const content = this.extractContent(response);

    if (!content) {
      throw new Error(
        'The model did not return any content.\n' +
          'This may occur if the model refused to respond or content was filtered.\n' +
          'Try adjusting your prompt or checking the provider\'s content policy.'
      );
    }

    return content;
  }

  /**
   * Performs a completion request with full control over parameters.
   *
   * @param params - The completion parameters
   * @returns The completion response object
   */
  async completion(params: Partial<CompletionOptions> = {}): Promise<any> {
    this.ensureProvider();

    if (!params || typeof params !== 'object') {
      throw new Error(
        'Parameters must be an object.\n' +
          "Example: client.completion({ messages: [{ role: 'user', content: 'Hello' }] })"
      );
    }

    const processedParams = this.processParams(params);
    return await this.provider!.completion(processedParams);
  }

  /**
   * Performs a chat completion request (alias for completion).
   *
   * @param params - The chat parameters
   * @returns The chat response object
   */
  async chat(params: Partial<CompletionOptions> = {}): Promise<any> {
    return this.completion(params);
  }

  /**
   * Performs an embedding request.
   *
   * @param params - The embedding parameters
   * @returns The embedding response object
   */
  async embed(params: any): Promise<any> {
    this.ensureProvider();

    if (!params || typeof params !== 'object') {
      throw new Error(
        'Parameters must be an object.\n' +
          "Example: client.embed({ model: 'text-embedding-ada-002', input: 'Hello' })"
      );
    }

    try {
      const processedParams = this.processParams(params);
      return await this.provider!.embedding(processedParams);
    } catch (error: any) {
      if (error.message?.includes('does not support embeddings')) {
        const providerName = this.provider!.constructor.name.replace('Provider', '');
        throw new Error(
          `${providerName} does not support embeddings.\n\n` +
            'Providers with embedding support:\n' +
            '  - OpenAI (text-embedding-ada-002, text-embedding-3-small, text-embedding-3-large)\n' +
            '  - Cohere (embed-english-v3.0, embed-multilingual-v3.0)\n\n' +
            "Example: new CosmosLlm('openai').embed({ model: 'text-embedding-ada-002', input: 'text' })"
        );
      }
      throw error;
    }
  }

  /**
   * Performs a streaming completion request.
   *
   * @param params - The streaming parameters
   * @param callback - Callback function to process each chunk
   */
  async stream(params: Partial<CompletionOptions>, callback: (chunk: any) => void): Promise<void> {
    this.ensureProvider();

    if (!params || typeof params !== 'object') {
      throw new Error(
        'Parameters must be an object.\n' +
          "Example: client.stream({ messages: [{ role: 'user', content: 'Hello' }] }, (chunk) => { ... })"
      );
    }

    if (!callback || typeof callback !== 'function') {
      throw new Error(
        'Streaming requires a callback function to process chunks.\n' +
          'Example: client.stream({ messages: [...] }, (chunk) => console.log(chunk))'
      );
    }

    try {
      const processedParams = this.processParams(params);
      await this.provider!.stream({ ...processedParams, onChunk: callback });
    } catch (error: any) {
      if (error.message?.includes('does not support streaming')) {
        const providerName = this.provider!.constructor.name.replace('Provider', '');
        throw new Error(
          `${providerName} does not support streaming.\n` +
            'Use the non-streaming methods instead:\n' +
            '  - client.completion({ messages: [...] })\n' +
            '  - client.chat({ messages: [...] })'
        );
      }
      throw error;
    }
  }

  /**
   * Checks if the provider supports streaming.
   *
   * @returns True if streaming is supported, false otherwise
   */
  canStream(): boolean {
    return this.provider?.supportsStreaming() ?? false;
  }

  /**
   * Sets the provider for the client (fluent interface).
   *
   * @param providerName - The name of the LLM provider
   * @returns This client instance for method chaining
   */
  withProvider(providerName: string): this {
    if (!providerName || providerName.trim() === '') {
      throw new Error(
        'Please specify a provider name.\n\n' +
          'Available providers: openai, anthropic\n\n' +
          'Example: client.withProvider(\'openai\')'
      );
    }

    this.initializeProvider(providerName, this.providerOptions);
    return this;
  }

  /**
   * Sets the model for subsequent requests (fluent interface).
   *
   * @param modelName - The model to use
   * @returns This client instance for method chaining
   */
  withModel(modelName: string): this {
    this.defaultModel = modelName;
    return this;
  }

  /**
   * Sets temperature for the next request (fluent interface).
   *
   * @param temp - The temperature value (0.0-2.0)
   * @returns This client instance for method chaining
   */
  withTemperature(temp: number): this {
    this.nextTemperature = temp;
    return this;
  }

  /**
   * Sets max tokens for the next request (fluent interface).
   *
   * @param tokens - Maximum tokens to generate
   * @returns This client instance for method chaining
   */
  withMaxTokens(tokens: number): this {
    this.nextMaxTokens = tokens;
    return this;
  }

  /**
   * Sets top_p for the next request (fluent interface).
   *
   * @param value - Top-p value (0.0-1.0)
   * @returns This client instance for method chaining
   */
  withTopP(value: number): this {
    this.nextTopP = value;
    return this;
  }

  /**
   * Sets stop sequences for the next request (fluent interface).
   *
   * @param sequences - Stop sequences
   * @returns This client instance for method chaining
   */
  withStop(sequences: string | string[]): this {
    this.nextStop = sequences;
    return this;
  }

  /**
   * Sets system message for the next request (fluent interface).
   *
   * @param message - System message content
   * @returns This client instance for method chaining
   */
  withSystem(message: string): this {
    this.nextSystem = message;
    return this;
  }

  /**
   * Creates a copy of the client with different configuration.
   *
   * @param options - New configuration options
   * @returns A new client instance with merged configuration
   */
  cloneWith(options: ClientOptions): Client {
    const providerName = this.provider?.constructor.name.toLowerCase().replace('provider', '');
    return new Client(providerName, {
      ...this.providerOptions,
      ...options,
      model: options.model || this.defaultModel,
    });
  }

  /**
   * Processes parameters by merging defaults and fluent settings.
   *
   * @param params - The base parameters
   * @returns The processed parameters
   */
  private processParams(params: any): any {
    const processed: any = {
      ...(this.defaultModel ? { model: this.defaultModel } : {}),
      ...params,
    };

    // Apply fluent interface settings if present
    if (this.nextTemperature !== undefined) {
      processed.temperature = this.nextTemperature;
    }
    if (this.nextMaxTokens !== undefined) {
      processed.maxTokens = this.nextMaxTokens;
    }
    if (this.nextTopP !== undefined) {
      processed.topP = this.nextTopP;
    }
    if (this.nextStop !== undefined) {
      processed.stop = this.nextStop;
    }
    if (this.nextSystem !== undefined) {
      // Prepend system message to messages array
      if (processed.messages) {
        processed.messages = [
          { role: 'system', content: this.nextSystem },
          ...processed.messages,
        ];
      }
    }

    // Clear one-time settings after use
    this.clearFluentSettings();

    return processed;
  }

  /**
   * Clears fluent interface settings.
   */
  private clearFluentSettings(): void {
    this.nextTemperature = undefined;
    this.nextMaxTokens = undefined;
    this.nextTopP = undefined;
    this.nextStop = undefined;
    this.nextSystem = undefined;
  }

  /**
   * Ensures provider is initialized before making requests.
   *
   * @throws Error if provider is not set
   */
  private ensureProvider(): void {
    if (!this.provider) {
      throw new Error(
        'No provider set. Please set a provider first.\n\n' +
          'Available providers: openai, anthropic\n\n' +
          "Example: client.withProvider('openai').complete('Hello')"
      );
    }
  }

  /**
   * Extracts content from a provider response.
   *
   * @param response - The provider response
   * @returns The extracted content string
   */
  private extractContent(response: any): string | null {
    // Handle OpenAI-style responses
    if (response.choices && Array.isArray(response.choices) && response.choices.length > 0) {
      const choice = response.choices[0];
      return choice.message?.content || choice.text || null;
    }

    // Handle Anthropic-style responses
    if (response.content && Array.isArray(response.content)) {
      const textBlocks = response.content
        .filter((block: any) => block.type === 'text')
        .map((block: any) => block.text);
      return textBlocks.join(' ') || null;
    }

    // Handle direct content field
    if (response.content) {
      return response.content;
    }

    return null;
  }
}
