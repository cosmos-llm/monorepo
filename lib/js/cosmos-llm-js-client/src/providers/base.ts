/**
 * Abstract base class for all LLM providers.
 *
 * This class defines the common interface that all LLM provider implementations must follow.
 * It provides default implementations for common functionality and defines abstract methods
 * that subclasses must implement.
 */

import { AuthenticationError } from '../errors';

export interface CompletionOptions {
  model: string;
  messages: Array<{ role: string; content: string; [key: string]: any }>;
  temperature?: number;
  maxTokens?: number;
  topP?: number;
  frequencyPenalty?: number;
  presencePenalty?: number;
  stop?: string | string[];
  [key: string]: any;
}

export interface EmbeddingOptions {
  model: string;
  input: string | string[];
  [key: string]: any;
}

export interface StreamOptions extends CompletionOptions {
  onChunk: (chunk: any) => void;
}

/**
 * Base class for all LLM providers.
 */
export abstract class BaseProvider {
  protected apiKey: string | undefined;

  constructor(apiKey?: string) {
    this.apiKey = apiKey || this.getDefaultApiKey();
  }

  /**
   * Gets the default API key from configuration or environment.
   * Subclasses must implement this method.
   *
   * @returns The default API key or undefined
   */
  protected abstract getDefaultApiKey(): string | undefined;

  /**
   * Performs a completion request.
   * Subclasses must implement this method.
   *
   * @param options - The completion options
   * @returns The completion response
   */
  abstract completion(options: CompletionOptions): Promise<any>;

  /**
   * Retrieves the list of available models.
   * Subclasses must implement this method.
   *
   * @returns Array of available model names
   */
  abstract models(): Promise<string[]>;

  /**
   * Checks if this provider supports streaming.
   *
   * @returns True if streaming is supported, false otherwise
   */
  supportsStreaming(): boolean {
    return false;
  }

  /**
   * Performs a streaming completion request.
   * Subclasses that support streaming should override this method.
   *
   * @param options - The streaming options
   */
  async stream(_options: StreamOptions): Promise<void> {
    throw new Error(
      `${this.constructor.name} does not support streaming. ` +
        'Use the non-streaming methods instead: completion()'
    );
  }

  /**
   * Performs an embedding request.
   * Subclasses that support embeddings should override this method.
   *
   * @param options - The embedding options
   * @returns The embedding response
   */
  async embedding(_options: EmbeddingOptions): Promise<any> {
    throw new Error(
      `${this.constructor.name} does not support embeddings. ` +
        'Try using a provider that supports embeddings like OpenAI or Cohere.'
    );
  }

  /**
   * Validates that the API key is configured.
   *
   * @throws AuthenticationError if API key is not configured
   */
  protected validateApiKey(): void {
    if (!this.apiKey || this.apiKey.trim() === '') {
      const providerName = this.constructor.name;
      const envVar = `CLLM__${providerName.toUpperCase()}__API_KEY`;

      throw new AuthenticationError(
        `API key required for ${providerName}.\n\n` +
          'Set it using one of these methods:\n' +
          `  1. Environment variable: export ${envVar}=your-key\n` +
          `  2. Configuration: config.${providerName.toLowerCase()}.apiKey = 'your-key'\n` +
          `  3. Client initialization: new CosmosLlm('${providerName.toLowerCase()}', { apiKey: 'your-key' })`
      );
    }
  }

  /**
   * Validates that required parameters are present.
   *
   * @param options - The options object to validate
   * @param requiredParams - Array of required parameter names
   * @throws Error if any required parameters are missing
   */
  protected validateRequiredParams(
    options: any,
    requiredParams: string[]
  ): void {
    const missing = requiredParams.filter((param) => {
      const value = options[param];
      return (
        value === undefined ||
        value === null ||
        (typeof value === 'string' && value.trim() === '') ||
        (Array.isArray(value) && value.length === 0)
      );
    });

    if (missing.length > 0) {
      throw new Error(
        `Missing required parameters: ${missing.join(', ')}. ` +
          'Please provide these parameters in your request.'
      );
    }
  }

  /**
   * Validates that a parameter is within a specified range.
   *
   * @param value - The value to validate
   * @param paramName - The parameter name for error messages
   * @param min - The minimum allowed value (inclusive)
   * @param max - The maximum allowed value (inclusive)
   * @throws Error if the value is outside the allowed range
   */
  protected validateRange(
    value: number | undefined,
    paramName: string,
    min: number,
    max: number
  ): void {
    if (value === undefined || value === null) {
      return; // Allow undefined/null values
    }

    if (value < min || value > max) {
      throw new Error(
        `The value ${value} for '${paramName}' is outside the allowed range. ` +
          `Please use a value between ${min} and ${max}.`
      );
    }
  }
}
