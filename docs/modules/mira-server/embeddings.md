<!-- docs/modules/mira-server/embeddings.md -->
# embeddings

OpenAI embedding client for generating vector embeddings used in semantic search and memory recall.

## Overview

Wraps the OpenAI text-embedding-3-small API to produce 1536-dimensional float vectors. The `EmbeddingClient` facade delegates to `OpenAiEmbeddings` for HTTP calls, batching, retry logic, and usage tracking. Created from `ApiKeys` + `EmbeddingsConfig` at startup and shared via `Arc` across the server.

## Key Types

- `EmbeddingClient` -- Top-level facade; created via `from_config()` or `from_env()`
- `OpenAiEmbeddings` -- Low-level OpenAI API client with retry, batching, and usage recording
- `OpenAiEmbeddingModel` -- Enum of supported models (TextEmbedding3Small, TextEmbedding3Large)

## Key Functions

- `embed(text)` -- Embed a single text string
- `embed_batch(texts)` -- Batch embed up to 256 texts per request, with parallel chunking for larger sets
- `dimensions()` -- Returns configured embedding dimensions (default: 1536)

## Sub-modules

| Module | Purpose |
|--------|---------|
| `openai` | OpenAI API client with retry, batching, usage tracking |

## Architecture Notes

Usage is recorded to the `embedding_usage` table via `DatabasePool` for cost tracking. Texts exceeding 32K characters are silently truncated at a UTF-8 boundary before sending to the API. Pending embedding queue operations live in `db/embeddings.rs`, not here.
