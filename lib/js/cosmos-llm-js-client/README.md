# Cosmos-LLM

Cosmos-LLM is a JavaScript/TypeScript library providing a unified interface for interacting with multiple Large Language Model APIs. It simplifies the integration of AI capabilities into JavaScript applications by offering a consistent way to access various LLM providers.

Part of the Cosmos LLM series. Enterprise support available from [Durable Programming](https://durableprogramming.com).

## Installation

```bash
npm install cosmos-llm
```

Or with yarn:

```bash
yarn add cosmos-llm
```

## Quick Start

### Simple Completion

```typescript
import { CosmosLlm } from 'cosmos-llm';

// Quick and simple - just provide an API key and model
const client = new CosmosLlm('openai', { apiKey: 'your-api-key', model: 'gpt-4' });
const response = await client.complete('What is the capital of France?');
console.log(response); // => "The capital of France is Paris."
```

### Using Global Configuration

```typescript
import { CosmosLlm, configure } from 'cosmos-llm';

// Configure once, use everywhere
configure((config) => {
  config.openai.apiKey = 'your-openai-api-key';
  config.anthropic.apiKey = 'your-anthropic-api-key';
});

// Create clients without passing API keys
const client = new CosmosLlm('openai', { model: 'gpt-4' });
const response = await client.complete('Hello, world!');
console.log(response);
```

### Chat Conversations

```typescript
const client = new CosmosLlm('openai', { model: 'gpt-4' });

const response = await client.chat({
  messages: [
    { role: 'system', content: 'You are a helpful assistant.' },
    { role: 'user', content: 'What is TypeScript?' }
  ]
});
console.log(response.choices[0].message.content);
```

### Streaming Responses

```typescript
const client = new CosmosLlm('openai', { model: 'gpt-4' });

await client.stream(
  { messages: [{ role: 'user', content: 'Count to 10' }] },
  (chunk) => {
    const content = chunk.choices?.[0]?.delta?.content;
    if (content) process.stdout.write(content);
  }
);
```

### Embeddings

```typescript
const client = new CosmosLlm('openai');

const response = await client.embed({
  model: 'text-embedding-ada-002',
  input: 'JavaScript is a dynamic programming language'
});

const embedding = response.data[0].embedding;
console.log(`Vector dimensions: ${embedding.length}`);
```

## Features

- **Unified Interface**: Consistent API across multiple LLM providers
- **Multiple Providers**: OpenAI, Anthropic, and more (expandable)
- **Streaming Support**: Real-time streaming responses
- **Embeddings**: Generate text embeddings for semantic search
- **Configuration Management**: Flexible configuration via environment variables or code
- **Error Handling**: Comprehensive error types for precise handling
- **TypeScript Support**: Full TypeScript type definitions included

## Supported Providers

Cosmos-LLM currently supports the following LLM providers:

- **OpenAI** - GPT-3.5, GPT-4, GPT-4 Turbo, and embeddings
- **Anthropic** - Claude 3 (Opus, Sonnet, Haiku)

Additional providers can be added following the same pattern.

## Configuration

### Environment Variables

Set API keys using environment variables with the `CLLM__` prefix:

```bash
export CLLM__OPENAI__API_KEY=your-openai-key
export CLLM__ANTHROPIC__API_KEY=your-anthropic-key
```

### Programmatic Configuration

Configure API keys and settings in your code:

```typescript
import { configure } from 'cosmos-llm';

configure((config) => {
  config.openai.apiKey = 'sk-...';
  config.anthropic.apiKey = 'sk-ant-...';
  config.defaultProvider = 'openai';
});
```

### Per-Client Configuration

Pass configuration directly when creating a client:

```typescript
const client = new CosmosLlm('openai', {
  apiKey: 'your-api-key',
  model: 'gpt-4'
});
```

## API Reference

### Client Methods

#### `new CosmosLlm(provider, options)`

Creates a new LLM client for the specified provider.

**Parameters:**
- `provider` (string) - Provider name ('openai', 'anthropic', etc.)
- `options` (object) - Configuration options
  - `model` - Default model to use
  - `apiKey` - API key for authentication
  - Other provider-specific options

**Returns:** `CosmosLlm` instance

#### `complete(text, opts?)`

Performs a simple text completion with minimal configuration.

**Parameters:**
- `text` (string) - Input text to complete
- `opts` (object) - Additional options (reserved for future use)

**Returns:** Promise<string> with the completion text

**Example:**
```typescript
const response = await client.complete('Explain quantum computing in one sentence');
console.log(response);
```

#### `completion(params)`

Performs a completion request with full control over parameters.

**Parameters:**
- `params` (object) - Completion parameters
  - `model` - Model to use (overrides default)
  - `messages` - Array of message objects with role and content
  - `temperature` - Sampling temperature (0.0-2.0)
  - `maxTokens` - Maximum tokens to generate
  - Other provider-specific parameters

**Returns:** Promise<object> with completion data

**Example:**
```typescript
const response = await client.completion({
  messages: [
    { role: 'system', content: 'You are a helpful coding assistant.' },
    { role: 'user', content: 'Write a TypeScript function to reverse a string' }
  ],
  temperature: 0.7,
  maxTokens: 500
});
```

#### `chat(params)`

Alias for `completion` - performs a chat completion request.

#### `stream(params, callback)`

Performs a streaming completion request, calling the callback with each chunk.

**Parameters:**
- `params` (object) - Same as completion
- `callback` (function) - Function to process each chunk

**Example:**
```typescript
await client.stream(
  { messages: [{ role: 'user', content: 'Write a story' }] },
  (chunk) => {
    const content = chunk.choices?.[0]?.delta?.content;
    if (content) process.stdout.write(content);
  }
);
```

#### `embed(params)`

Generates embeddings for the given text.

**Parameters:**
- `params` (object) - Embedding parameters
  - `model` - Embedding model to use
  - `input` - Text or array of texts to embed

**Returns:** Promise<object> with embedding vectors

**Example:**
```typescript
const response = await client.embed({
  model: 'text-embedding-ada-002',
  input: ['First text', 'Second text']
});

const embeddings = response.data.map(item => item.embedding);
```

## Advanced Usage

### Fluent API with Method Chaining

```typescript
const client = new CosmosLlm('openai', { model: 'gpt-3.5-turbo' });

const result = await client
  .withModel('gpt-4')
  .withTemperature(0.7)
  .withMaxTokens(500)
  .complete('Write a haiku about TypeScript');

console.log(result);
```

### Cloning Clients with Different Settings

```typescript
const baseClient = new CosmosLlm('openai', { model: 'gpt-3.5-turbo' });

// Create variant clients for different use cases
const fastClient = baseClient.cloneWith({ model: 'gpt-3.5-turbo' });
const powerfulClient = baseClient.cloneWith({ model: 'gpt-4' });

// Use them for different tasks
const summary = await fastClient.complete('Summarize: ...');
const analysis = await powerfulClient.complete('Analyze in depth: ...');
```

### Static Helper Methods

```typescript
// Quick one-liner completions
const result = await CosmosLlm.quickComplete('Hello!', {
  provider: 'openai',
  model: 'gpt-4',
  apiKey: 'sk-...'
});

// Quick chat
const response = await CosmosLlm.quickChat(
  [{ role: 'user', content: 'Hello!' }],
  { provider: 'openai', model: 'gpt-4' }
);
```

## Examples

### Building a Simple Chatbot

```typescript
import { CosmosLlm } from 'cosmos-llm';
import * as readline from 'readline';

const client = new CosmosLlm('openai', { model: 'gpt-4' });
const conversation = [
  { role: 'system', content: 'You are a helpful assistant.' }
];

const rl = readline.createInterface({
  input: process.stdin,
  output: process.stdout
});

async function chat() {
  rl.question('You: ', async (input) => {
    if (input.toLowerCase() === 'exit') {
      rl.close();
      return;
    }

    conversation.push({ role: 'user', content: input });

    const response = await client.chat({ messages: conversation });
    const assistantMessage = response.choices[0].message.content;

    conversation.push({ role: 'assistant', content: assistantMessage });
    console.log(`Assistant: ${assistantMessage}`);

    chat();
  });
}

chat();
```

### Streaming Real-Time Translation

```typescript
import { CosmosLlm } from 'cosmos-llm';

const client = new CosmosLlm('openai', { model: 'gpt-4' });

async function translateStreaming(text: string, targetLanguage: string) {
  const messages = [
    {
      role: 'user',
      content: `Translate to ${targetLanguage}: ${text}`
    }
  ];

  process.stdout.write('Translation: ');
  await client.stream({ messages }, (chunk) => {
    const content = chunk.choices?.[0]?.delta?.content;
    if (content) process.stdout.write(content);
  });
  console.log();
}

translateStreaming('Hello, how are you?', 'Spanish');
```

## Error Handling

The library provides specific error types for precise error handling:

```typescript
import { CosmosLlm, AuthenticationError, RateLimitError, InvalidRequestError } from 'cosmos-llm';

try {
  const client = new CosmosLlm('openai', { model: 'gpt-4' });
  const response = await client.complete('Hello!');
} catch (error) {
  if (error instanceof AuthenticationError) {
    console.error('Authentication failed. Check your API key.');
  } else if (error instanceof RateLimitError) {
    console.error('Rate limit exceeded. Wait and retry.');
  } else if (error instanceof InvalidRequestError) {
    console.error('Invalid request parameters.');
  } else {
    console.error('An error occurred:', error);
  }
}
```

## Contributing

Bug reports and pull requests are welcome on GitHub at https://github.com/cosmos-llm/cosmos-llm-js.

## License

The library is available as open source under the terms of the [MIT License](https://opensource.org/licenses/MIT).
