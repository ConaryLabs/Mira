# llm/openai_compat

Shared OpenAI-compatible request/response handling. Used by DeepSeek and other providers that implement the OpenAI chat completions API format.

## Key Exports

- `ChatRequest` - OpenAI-format chat completion request
- `ChatResponse` - OpenAI-format chat completion response
- `ResponseChoice` - Individual response choice
- `parse_chat_response()` - Parse raw response into structured data

## Sub-modules

| Module | Purpose |
|--------|---------|
| `request` | Request type construction |
| `response` | Response parsing |

## Usage

Providers like DeepSeek, Zhipu, and Ollama build their requests using these shared types rather than implementing their own serialization, reducing code duplication.
