/**
 * Unit tests for HttpClient class.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { HttpClient } from './http-client';
import fetch from 'node-fetch';

vi.mock('node-fetch', () => ({
  default: vi.fn(),
}));

describe('HttpClient', () => {
  let client: HttpClient;
  const baseUrl = 'https://api.example.com';

  beforeEach(() => {
    vi.clearAllMocks();
    client = new HttpClient(baseUrl);
  });

  describe('constructor', () => {
    it('should initialize with the correct base URL', () => {
      expect(client).toBeDefined();
    });
  });

  describe('get', () => {
    it('should make a GET request to the correct URL', async () => {
      const mockResponse = {
        status: 200,
        text: vi.fn().mockResolvedValue(JSON.stringify({ success: true })),
      };
      vi.mocked(fetch).mockResolvedValue(mockResponse as any);

      await client.get('test/path');

      expect(fetch).toHaveBeenCalledWith(
        `${baseUrl}/test/path`,
        expect.objectContaining({
          method: 'GET',
          headers: expect.objectContaining({
            'Content-Type': 'application/json',
          }),
        })
      );
    });

    it('should include custom headers in the request', async () => {
      const mockResponse = {
        status: 200,
        text: vi.fn().mockResolvedValue('{}'),
      };
      vi.mocked(fetch).mockResolvedValue(mockResponse as any);

      const customHeaders = { Authorization: 'Bearer token' };
      await client.get('test', customHeaders);

      expect(fetch).toHaveBeenCalledWith(
        expect.any(String),
        expect.objectContaining({
          headers: expect.objectContaining({
            'Content-Type': 'application/json',
            Authorization: 'Bearer token',
          }),
        })
      );
    });

    it('should return the parsed response', async () => {
      const responseData = { result: 'success' };
      const mockResponse = {
        status: 200,
        text: vi.fn().mockResolvedValue(JSON.stringify(responseData)),
      };
      vi.mocked(fetch).mockResolvedValue(mockResponse as any);

      const result = await client.get('test');

      expect(result.status).toBe(200);
      expect(result.body).toEqual(responseData);
    });

    it('should handle empty response body', async () => {
      const mockResponse = {
        status: 200,
        text: vi.fn().mockResolvedValue(''),
      };
      vi.mocked(fetch).mockResolvedValue(mockResponse as any);

      const result = await client.get('test');

      expect(result.body).toEqual({});
    });

    it('should handle non-JSON response', async () => {
      const mockResponse = {
        status: 200,
        text: vi.fn().mockResolvedValue('plain text response'),
      };
      vi.mocked(fetch).mockResolvedValue(mockResponse as any);

      const result = await client.get('test');

      expect(result.body).toBe('plain text response');
    });
  });

  describe('post', () => {
    it('should make a POST request with the correct body', async () => {
      const mockResponse = {
        status: 201,
        text: vi.fn().mockResolvedValue(JSON.stringify({ id: 123 })),
      };
      vi.mocked(fetch).mockResolvedValue(mockResponse as any);

      const requestBody = { name: 'test', value: 42 };
      await client.post('test/path', requestBody);

      expect(fetch).toHaveBeenCalledWith(
        `${baseUrl}/test/path`,
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify(requestBody),
        })
      );
    });

    it('should include custom headers in the request', async () => {
      const mockResponse = {
        status: 200,
        text: vi.fn().mockResolvedValue('{}'),
      };
      vi.mocked(fetch).mockResolvedValue(mockResponse as any);

      const customHeaders = { 'X-Custom-Header': 'value' };
      await client.post('test', {}, customHeaders);

      expect(fetch).toHaveBeenCalledWith(
        expect.any(String),
        expect.objectContaining({
          headers: expect.objectContaining({
            'Content-Type': 'application/json',
            'X-Custom-Header': 'value',
          }),
        })
      );
    });

    it('should return the parsed response', async () => {
      const responseData = { id: 456, created: true };
      const mockResponse = {
        status: 201,
        text: vi.fn().mockResolvedValue(JSON.stringify(responseData)),
      };
      vi.mocked(fetch).mockResolvedValue(mockResponse as any);

      const result = await client.post('test', { data: 'test' });

      expect(result.status).toBe(201);
      expect(result.body).toEqual(responseData);
    });

    it('should handle error responses', async () => {
      const errorData = { error: 'Bad request' };
      const mockResponse = {
        status: 400,
        text: vi.fn().mockResolvedValue(JSON.stringify(errorData)),
      };
      vi.mocked(fetch).mockResolvedValue(mockResponse as any);

      const result = await client.post('test', {});

      expect(result.status).toBe(400);
      expect(result.body).toEqual(errorData);
    });
  });

  describe('postStream', () => {
    it('should make a streaming POST request', async () => {
      const mockBody = {
        [Symbol.asyncIterator]: async function* () {
          yield Buffer.from('data: {"delta":"hello"}\n\n');
          yield Buffer.from('data: [DONE]\n\n');
        },
      };

      const mockResponse = {
        status: 200,
        ok: true,
        body: mockBody,
      };
      vi.mocked(fetch).mockResolvedValue(mockResponse as any);

      const onChunk = vi.fn();
      const requestBody = { prompt: 'test' };

      await client.postStream('test/stream', {
        headers: {},
        body: requestBody,
        onChunk,
      });

      expect(fetch).toHaveBeenCalledWith(
        `${baseUrl}/test/stream`,
        expect.objectContaining({
          method: 'POST',
          headers: expect.objectContaining({
            Accept: 'text/event-stream',
          }),
          body: JSON.stringify(requestBody),
        })
      );
    });

    it('should process streaming chunks and call onChunk callback', async () => {
      const mockBody = {
        [Symbol.asyncIterator]: async function* () {
          yield Buffer.from('data: {"text":"hello"}\n');
          yield Buffer.from('data: {"text":"world"}\n');
          yield Buffer.from('data: [DONE]\n\n');
        },
      };

      const mockResponse = {
        status: 200,
        ok: true,
        body: mockBody,
      };
      vi.mocked(fetch).mockResolvedValue(mockResponse as any);

      const onChunk = vi.fn();

      await client.postStream('test', {
        headers: {},
        body: {},
        onChunk,
      });

      expect(onChunk).toHaveBeenCalledWith({ text: 'hello' });
      expect(onChunk).toHaveBeenCalledWith({ text: 'world' });
    });

    it('should skip [DONE] messages', async () => {
      const mockBody = {
        [Symbol.asyncIterator]: async function* () {
          yield Buffer.from('data: {"text":"test"}\n');
          yield Buffer.from('data: [DONE]\n');
        },
      };

      const mockResponse = {
        status: 200,
        ok: true,
        body: mockBody,
      };
      vi.mocked(fetch).mockResolvedValue(mockResponse as any);

      const onChunk = vi.fn();

      await client.postStream('test', {
        headers: {},
        body: {},
        onChunk,
      });

      expect(onChunk).toHaveBeenCalledTimes(1);
      expect(onChunk).toHaveBeenCalledWith({ text: 'test' });
    });

    it('should skip malformed JSON chunks', async () => {
      const mockBody = {
        [Symbol.asyncIterator]: async function* () {
          yield Buffer.from('data: {"valid":"json"}\n');
          yield Buffer.from('data: {invalid json}\n');
          yield Buffer.from('data: {"also":"valid"}\n');
        },
      };

      const mockResponse = {
        status: 200,
        ok: true,
        body: mockBody,
      };
      vi.mocked(fetch).mockResolvedValue(mockResponse as any);

      const onChunk = vi.fn();

      await client.postStream('test', {
        headers: {},
        body: {},
        onChunk,
      });

      expect(onChunk).toHaveBeenCalledTimes(2);
      expect(onChunk).toHaveBeenCalledWith({ valid: 'json' });
      expect(onChunk).toHaveBeenCalledWith({ also: 'valid' });
    });

    it('should handle error responses in streaming', async () => {
      const errorBody = { error: 'Unauthorized' };
      const mockResponse = {
        status: 401,
        ok: false,
        text: vi.fn().mockResolvedValue(JSON.stringify(errorBody)),
      };
      vi.mocked(fetch).mockResolvedValue(mockResponse as any);

      const onChunk = vi.fn();

      const result = await client.postStream('test', {
        headers: {},
        body: {},
        onChunk,
      });

      expect(result.status).toBe(401);
      expect(result.body).toEqual(errorBody);
      expect(onChunk).not.toHaveBeenCalled();
    });

    it('should throw error if response body is null', async () => {
      const mockResponse = {
        status: 200,
        ok: true,
        body: null,
      };
      vi.mocked(fetch).mockResolvedValue(mockResponse as any);

      await expect(
        client.postStream('test', {
          headers: {},
          body: {},
          onChunk: vi.fn(),
        })
      ).rejects.toThrow('Response body is null');
    });

    it('should handle multi-line chunks correctly', async () => {
      const mockBody = {
        [Symbol.asyncIterator]: async function* () {
          yield Buffer.from('data: {"part');
          yield Buffer.from('1":"value"}\ndata: {"part2":"value2"}\n');
        },
      };

      const mockResponse = {
        status: 200,
        ok: true,
        body: mockBody,
      };
      vi.mocked(fetch).mockResolvedValue(mockResponse as any);

      const onChunk = vi.fn();

      await client.postStream('test', {
        headers: {},
        body: {},
        onChunk,
      });

      expect(onChunk).toHaveBeenCalledWith({ part1: 'value' });
      expect(onChunk).toHaveBeenCalledWith({ part2: 'value2' });
    });
  });
});
