/**
 * Configuration class for managing LLM provider settings and API keys.
 *
 * This class provides a centralized configuration management system for the Cosmos LLM library.
 * It supports dynamic provider configuration and automatic loading from environment variables
 * using the `CLLM__` prefix pattern.
 *
 * ## Basic Usage
 *
 * ```typescript
 * const config = new Configuration();
 *
 * // Configure providers
 * config.openai = { apiKey: 'sk-...', model: 'gpt-4' };
 * config.anthropic.apiKey = 'sk-ant-...';
 *
 * // Set default provider
 * config.defaultProvider = 'anthropic';
 * ```
 *
 * ## Environment Variable Configuration
 *
 * Configuration can be loaded from environment variables using the `CLLM__` prefix:
 *
 * ```bash
 * export CLLM__OPENAI__API_KEY=sk-your-key
 * export CLLM__ANTHROPIC__API_KEY=sk-ant-your-key
 * export CLLM__OPENAI__MODEL=gpt-4
 * ```
 */

export interface ProviderConfig {
  apiKey?: string;
  model?: string;
  [key: string]: any;
}

export class Configuration {
  public defaultProvider: string = 'openai';
  private providers: Map<string, ProviderConfig> = new Map();

  constructor() {
    this.loadFromEnv();
  }

  /**
   * Gets the configuration for a specific provider.
   *
   * @param provider - The provider name
   * @returns The provider configuration object
   */
  public getProvider(provider: string): ProviderConfig {
    if (!this.providers.has(provider)) {
      this.providers.set(provider, {});
    }
    return this.providers.get(provider)!;
  }

  /**
   * Sets the configuration for a specific provider.
   *
   * @param provider - The provider name
   * @param config - The configuration object
   */
  public setProvider(provider: string, config: ProviderConfig): void {
    this.providers.set(provider, { ...this.getProvider(provider), ...config });
  }

  /**
   * Clears all provider configurations and resets to defaults.
   */
  public clear(): void {
    this.providers.clear();
    this.defaultProvider = 'openai';
    this.loadFromEnv();
  }

  /**
   * Loads configuration from environment variables.
   *
   * This method scans all environment variables for those starting with the
   * `CLLM__` prefix and automatically configures provider settings based on
   * the variable names. The format is `CLLM__PROVIDER__SETTING=value`.
   *
   * For example:
   * - `CLLM__OPENAI__API_KEY=sk-...` sets the API key for OpenAI
   * - `CLLM__ANTHROPIC__MODEL=claude-3` sets the default model for Anthropic
   */
  private loadFromEnv(): void {
    for (const [key, value] of Object.entries(process.env)) {
      if (!key.startsWith('CLLM__')) {
        continue;
      }

      const parts = key.split('__');
      if (parts.length < 3) {
        continue;
      }

      const provider = parts[1].toLowerCase();
      const setting = this.camelCase(parts[2].toLowerCase());

      const config = this.getProvider(provider);
      config[setting] = value;
    }
  }

  /**
   * Converts snake_case or SCREAMING_SNAKE_CASE to camelCase.
   *
   * @param str - The string to convert
   * @returns The camelCase version
   */
  private camelCase(str: string): string {
    return str.replace(/_([a-z])/g, (_, letter) => letter.toUpperCase());
  }

  /**
   * Dynamic getter for provider configurations.
   * Allows accessing configs like: config.openai
   */
  get openai(): ProviderConfig {
    return this.getProvider('openai');
  }

  set openai(config: ProviderConfig) {
    this.setProvider('openai', config);
  }

  get anthropic(): ProviderConfig {
    return this.getProvider('anthropic');
  }

  set anthropic(config: ProviderConfig) {
    this.setProvider('anthropic', config);
  }

  get google(): ProviderConfig {
    return this.getProvider('google');
  }

  set google(config: ProviderConfig) {
    this.setProvider('google', config);
  }

  get cohere(): ProviderConfig {
    return this.getProvider('cohere');
  }

  set cohere(config: ProviderConfig) {
    this.setProvider('cohere', config);
  }

  get mistral(): ProviderConfig {
    return this.getProvider('mistral');
  }

  set mistral(config: ProviderConfig) {
    this.setProvider('mistral', config);
  }

  get groq(): ProviderConfig {
    return this.getProvider('groq');
  }

  set groq(config: ProviderConfig) {
    this.setProvider('groq', config);
  }

  get fireworks(): ProviderConfig {
    return this.getProvider('fireworks');
  }

  set fireworks(config: ProviderConfig) {
    this.setProvider('fireworks', config);
  }

  get together(): ProviderConfig {
    return this.getProvider('together');
  }

  set together(config: ProviderConfig) {
    this.setProvider('together', config);
  }

  get deepseek(): ProviderConfig {
    return this.getProvider('deepseek');
  }

  set deepseek(config: ProviderConfig) {
    this.setProvider('deepseek', config);
  }

  get openrouter(): ProviderConfig {
    return this.getProvider('openrouter');
  }

  set openrouter(config: ProviderConfig) {
    this.setProvider('openrouter', config);
  }

  get perplexity(): ProviderConfig {
    return this.getProvider('perplexity');
  }

  set perplexity(config: ProviderConfig) {
    this.setProvider('perplexity', config);
  }

  get xai(): ProviderConfig {
    return this.getProvider('xai');
  }

  set xai(config: ProviderConfig) {
    this.setProvider('xai', config);
  }

  get azureopenai(): ProviderConfig {
    return this.getProvider('azureopenai');
  }

  set azureopenai(config: ProviderConfig) {
    this.setProvider('azureopenai', config);
  }

  get huggingface(): ProviderConfig {
    return this.getProvider('huggingface');
  }

  set huggingface(config: ProviderConfig) {
    this.setProvider('huggingface', config);
  }

  get opencode(): ProviderConfig {
    return this.getProvider('opencode');
  }

  set opencode(config: ProviderConfig) {
    this.setProvider('opencode', config);
  }
}
