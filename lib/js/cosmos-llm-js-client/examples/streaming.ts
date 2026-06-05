/**
 * Streaming example using Cosmos LLM.
 *
 * This example demonstrates how to use streaming to get
 * real-time responses from the LLM.
 */

import { CosmosLlm } from '../src';

async function main() {
  const client = new CosmosLlm('openai', {
    apiKey: process.env.OPENAI_API_KEY,
    model: 'gpt-4'
  });

  console.log('Streaming response:');
  console.log('-------------------');

  await client.stream(
    {
      messages: [
        { role: 'user', content: 'Write a short poem about TypeScript' }
      ]
    },
    (chunk) => {
      const content = chunk.choices?.[0]?.delta?.content;
      if (content) {
        process.stdout.write(content);
      }
    }
  );

  console.log('\n-------------------');
  console.log('Stream complete!');
}

main().catch(console.error);
