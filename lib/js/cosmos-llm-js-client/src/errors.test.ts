/**
 * Unit tests for custom error classes.
 */

import { describe, it, expect } from 'vitest';
import {
  CosmosLlmError,
  APIError,
  AuthenticationError,
  RateLimitError,
  InvalidRequestError,
  ServerError,
} from './errors';

describe('Error Classes', () => {
  describe('CosmosLlmError', () => {
    it('should create an error with the correct message', () => {
      const error = new CosmosLlmError('Test error message');
      expect(error.message).toBe('Test error message');
    });

    it('should have the correct name', () => {
      const error = new CosmosLlmError('Test');
      expect(error.name).toBe('CosmosLlmError');
    });

    it('should be an instance of Error', () => {
      const error = new CosmosLlmError('Test');
      expect(error).toBeInstanceOf(Error);
    });

    it('should be an instance of CosmosLlmError', () => {
      const error = new CosmosLlmError('Test');
      expect(error).toBeInstanceOf(CosmosLlmError);
    });
  });

  describe('APIError', () => {
    it('should create an error with the correct message', () => {
      const error = new APIError('API error occurred');
      expect(error.message).toBe('API error occurred');
    });

    it('should have the correct name', () => {
      const error = new APIError('Test');
      expect(error.name).toBe('APIError');
    });

    it('should be an instance of CosmosLlmError', () => {
      const error = new APIError('Test');
      expect(error).toBeInstanceOf(CosmosLlmError);
    });

    it('should be an instance of APIError', () => {
      const error = new APIError('Test');
      expect(error).toBeInstanceOf(APIError);
    });

    it('should be throwable and catchable', () => {
      expect(() => {
        throw new APIError('Test error');
      }).toThrow(APIError);
    });
  });

  describe('AuthenticationError', () => {
    it('should create an error with the correct message', () => {
      const error = new AuthenticationError('Invalid API key');
      expect(error.message).toBe('Invalid API key');
    });

    it('should have the correct name', () => {
      const error = new AuthenticationError('Test');
      expect(error.name).toBe('AuthenticationError');
    });

    it('should be an instance of CosmosLlmError', () => {
      const error = new AuthenticationError('Test');
      expect(error).toBeInstanceOf(CosmosLlmError);
    });

    it('should be an instance of AuthenticationError', () => {
      const error = new AuthenticationError('Test');
      expect(error).toBeInstanceOf(AuthenticationError);
    });

    it('should be throwable and catchable', () => {
      expect(() => {
        throw new AuthenticationError('Invalid credentials');
      }).toThrow(AuthenticationError);
    });
  });

  describe('RateLimitError', () => {
    it('should create an error with the correct message', () => {
      const error = new RateLimitError('Rate limit exceeded');
      expect(error.message).toBe('Rate limit exceeded');
    });

    it('should have the correct name', () => {
      const error = new RateLimitError('Test');
      expect(error.name).toBe('RateLimitError');
    });

    it('should be an instance of CosmosLlmError', () => {
      const error = new RateLimitError('Test');
      expect(error).toBeInstanceOf(CosmosLlmError);
    });

    it('should be an instance of RateLimitError', () => {
      const error = new RateLimitError('Test');
      expect(error).toBeInstanceOf(RateLimitError);
    });

    it('should be throwable and catchable', () => {
      expect(() => {
        throw new RateLimitError('Too many requests');
      }).toThrow(RateLimitError);
    });
  });

  describe('InvalidRequestError', () => {
    it('should create an error with the correct message', () => {
      const error = new InvalidRequestError('Invalid parameters');
      expect(error.message).toBe('Invalid parameters');
    });

    it('should have the correct name', () => {
      const error = new InvalidRequestError('Test');
      expect(error.name).toBe('InvalidRequestError');
    });

    it('should be an instance of CosmosLlmError', () => {
      const error = new InvalidRequestError('Test');
      expect(error).toBeInstanceOf(CosmosLlmError);
    });

    it('should be an instance of InvalidRequestError', () => {
      const error = new InvalidRequestError('Test');
      expect(error).toBeInstanceOf(InvalidRequestError);
    });

    it('should be throwable and catchable', () => {
      expect(() => {
        throw new InvalidRequestError('Bad request');
      }).toThrow(InvalidRequestError);
    });
  });

  describe('ServerError', () => {
    it('should create an error with the correct message', () => {
      const error = new ServerError('Internal server error');
      expect(error.message).toBe('Internal server error');
    });

    it('should have the correct name', () => {
      const error = new ServerError('Test');
      expect(error.name).toBe('ServerError');
    });

    it('should be an instance of CosmosLlmError', () => {
      const error = new ServerError('Test');
      expect(error).toBeInstanceOf(CosmosLlmError);
    });

    it('should be an instance of ServerError', () => {
      const error = new ServerError('Test');
      expect(error).toBeInstanceOf(ServerError);
    });

    it('should be throwable and catchable', () => {
      expect(() => {
        throw new ServerError('Server error');
      }).toThrow(ServerError);
    });
  });

  describe('Error Hierarchy', () => {
    it('should catch all custom errors as CosmosLlmError', () => {
      const errors = [
        new APIError('test'),
        new AuthenticationError('test'),
        new RateLimitError('test'),
        new InvalidRequestError('test'),
        new ServerError('test'),
      ];

      errors.forEach((error) => {
        expect(error).toBeInstanceOf(CosmosLlmError);
      });
    });

    it('should preserve error stack traces', () => {
      const error = new APIError('Test error');
      expect(error.stack).toBeDefined();
      expect(error.stack).toContain('APIError');
    });

    it('should allow differentiating between error types', () => {
      const apiError = new APIError('API');
      const authError = new AuthenticationError('Auth');

      expect(apiError).toBeInstanceOf(APIError);
      expect(apiError).not.toBeInstanceOf(AuthenticationError);
      expect(authError).toBeInstanceOf(AuthenticationError);
      expect(authError).not.toBeInstanceOf(APIError);
    });
  });
});
