/**
 * Test setup and global configuration for Vitest.
 */

import { vi } from 'vitest';

// Mock environment variables
beforeEach(() => {
  // Clear all mocks before each test
  vi.clearAllMocks();

  // Reset environment variables
  delete process.env.OPENAI_API_KEY;
  delete process.env.ANTHROPIC_API_KEY;
  delete process.env.CLLM__OPENAI__API_KEY;
  delete process.env.CLLM__ANTHROPIC__API_KEY;
});
