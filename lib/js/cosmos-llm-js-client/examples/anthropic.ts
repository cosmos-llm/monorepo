/**
 * Anthropic Claude example using Cosmos LLM.
 *
 * This example demonstrates how to use Anthropic's Claude models.
 */

import { CosmosLlm } from '../src';

async function main() {
  const client = new CosmosLlm('anthropic', {
    apiKey: process.env.ANTHROPIC_API_KEY,
    model: 'claude-3-5-sonnet-20240620'
  });

  // Simple completion
  console.log('Simple completion:');
  const response = await client.complete('What is the meaning of life?');
  console.log(response);

  // Chat with system message
  console.log('\n\nChat with system message:');
  const chatResponse = await client.chat({
    messages: [
      { role: 'system', content: 'You are a philosophical AI assistant.' },
      { role: 'user', content: 'What is consciousness?' }
    ]
  });

  console.log(chatResponse.content[0].text);

  // Streaming example
  console.log('\n\nStreaming response:');
  await client.stream(
    {
      messages: [
        { role: 'user', content: 'Count from 1 to 10' }
      ]
    },
    (chunk) => {
      if (chunk.type === 'content_block_delta' && chunk.delta?.text) {
        process.stdout.write(chunk.delta.text);
      }
    }
  );

  console.log('\n\nDone!');
}

main().catch(console.error);
