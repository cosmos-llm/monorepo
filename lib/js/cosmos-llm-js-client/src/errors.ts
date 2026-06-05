/**
 * Custom error classes for the Cosmos LLM library.
 *
 * These error classes provide specific error types for different failure scenarios
 * when interacting with LLM APIs, allowing for precise error handling.
 */

/**
 * Base error class for all Cosmos LLM errors.
 */
export class CosmosLlmError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'CosmosLlmError';
    Object.setPrototypeOf(this, CosmosLlmError.prototype);
  }
}

/**
 * Error thrown when there's a general API error.
 */
export class APIError extends CosmosLlmError {
  constructor(message: string) {
    super(message);
    this.name = 'APIError';
    Object.setPrototypeOf(this, APIError.prototype);
  }
}

/**
 * Error thrown when authentication fails.
 */
export class AuthenticationError extends CosmosLlmError {
  constructor(message: string) {
    super(message);
    this.name = 'AuthenticationError';
    Object.setPrototypeOf(this, AuthenticationError.prototype);
  }
}

/**
 * Error thrown when rate limits are exceeded.
 */
export class RateLimitError extends CosmosLlmError {
  constructor(message: string) {
    super(message);
    this.name = 'RateLimitError';
    Object.setPrototypeOf(this, RateLimitError.prototype);
  }
}

/**
 * Error thrown when the request is invalid.
 */
export class InvalidRequestError extends CosmosLlmError {
  constructor(message: string) {
    super(message);
    this.name = 'InvalidRequestError';
    Object.setPrototypeOf(this, InvalidRequestError.prototype);
  }
}

/**
 * Error thrown when the server encounters an error.
 */
export class ServerError extends CosmosLlmError {
  constructor(message: string) {
    super(message);
    this.name = 'ServerError';
    Object.setPrototypeOf(this, ServerError.prototype);
  }
}
