/**
 * Fluent API example using Cosmos LLM.
 *
 * This example shows how to use the fluent interface
 * for method chaining and cleaner code.
 */

import { CosmosLlm } from '../src';

async function main() {
  const client = new CosmosLlm('openai', {
    apiKey: process.env.OPENAI_API_KEY,
    model: 'gpt-3.5-turbo'
  });

  // Use fluent interface to configure and make request
  const haiku = await client
    .withModel('gpt-4')
    .withTemperature(0.8)
    .withMaxTokens(100)
    .withSystem('You are a creative poet.')
    .complete('Write a haiku about coding');

  console.log('Haiku:');
  console.log(haiku);

  // Another example with different settings
  const analysis = await client
    .withTemperature(0.3)
    .withMaxTokens(500)
    .complete('Explain the benefits of TypeScript over JavaScript');

  console.log('\nAnalysis:');
  console.log(analysis);
}

main().catch(console.error);
