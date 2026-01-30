# llm/gemini

Google Gemini API client for chat and embedding generation.

## Key Export

`GeminiClient` - Implements the `LlmClient` trait for the Gemini API.

## Sub-modules

| Module | Purpose |
|--------|---------|
| `client` | Client implementation |
| `conversion` | Format conversion between Mira and Gemini types |
| `extraction` | Response parsing and content extraction |
| `types` | Gemini-specific request/response types |

## Features

- Chat completion with multi-turn conversations
- Embedding generation (used for semantic search via `gemini-embedding-001`)
- Tool calling support
- Native Gemini API format (not OpenAI-compatible)
