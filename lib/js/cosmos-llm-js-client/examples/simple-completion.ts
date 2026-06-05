/**
 * Simple completion example using Cosmos LLM.
 *
 * This example shows how to perform a basic text completion
 * with minimal configuration.
 */

import { CosmosLlm } from '../src';

async function main() {
  // Create a client with OpenAI
  const client = new CosmosLlm('openai', {
    apiKey: process.env.OPENAI_API_KEY,
    model: 'gpt-3.5-turbo'
  });

  // Perform a simple completion
  const response = await client.complete('What is the capital of France?');
  console.log('Response:', response);

  // Perform a completion with more control
  const detailedResponse = await client.completion({
    messages: [
      { role: 'system', content: 'You are a helpful geography assistant.' },
      { role: 'user', content: 'What is the capital of France?' }
    ],
    temperature: 0.7
  });

  console.log('\nDetailed response:', detailedResponse.choices[0].message.content);
}

main().catch(console.error);
