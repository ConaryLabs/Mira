<!-- docs/modules/mira-server/embeddings.md -->
# embeddings

OpenAI embedding client for generating vector embeddings used in semantic search and memory recall.

## Overview

Wraps embedding provider APIs (OpenAI text-embedding-3-small at 1536 dims, or Ollama models such as nomic-embed-text at 768 dims) to produce float vectors for semantic search and memory recall. The `EmbeddingClient` facade delegates to the active backend for HTTP calls, batching, retry logic, and usage tracking. Created from `ApiKeys` + `EmbeddingsConfig` at startup and shared via `Arc` across the server.

## Key Types

- `EmbeddingClient` -- Top-level facade; created via `from_config()`, `from_env()`, `from_config_with_http_client()`, or `from_env_with_http_client()`
- `OpenAiEmbeddings` -- Low-level OpenAI API client with retry, batching, and usage recording
- `OpenAiEmbeddingModel` -- Enum of supported models (TextEmbedding3Small, TextEmbedding3Large)

## Key Functions

- `embed(text)` -- Embed a single text string
- `embed_batch(texts)` -- Batch embed up to 256 texts per request, with parallel chunking for larger sets
- `dimensions()` -- Returns configured embedding dimensions (provider-dependent; 1536 for OpenAI, 768 for nomic-embed-text)
- `provider_id()` -- Returns provider identifier string
- `model_name()` -- Returns model name
- `set_project_id()` -- Set project context for usage tracking
- `inner()` -- Access the underlying `OpenAiEmbeddings` client
- `EMBEDDING_PROVIDER_KEY` -- Public constant for provider identification

## Sub-modules

| Module | Purpose |
|--------|---------|
| `openai` | OpenAI API client with retry, batching, usage tracking |

## Architecture Notes

Usage is recorded to the `embedding_usage` table via `DatabasePool` for cost tracking. Texts exceeding 32K characters are silently truncated at a UTF-8 boundary before sending to the API. Pending embedding queue operations live in `db/embeddings.rs`, not here.
