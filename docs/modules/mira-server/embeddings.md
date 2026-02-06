# embeddings

OpenAI embedding client for generating vector embeddings used in semantic search.

## Key Types

- `EmbeddingClient` — Manages embedding generation with task-type-aware dimensions
- `OpenAiEmbeddings` — OpenAI text-embedding-3-small API client

## Sub-modules

| Module | Purpose |
|--------|---------|
| `openai` | OpenAI API client implementation |

## Task-Type-Aware Embeddings

The module uses different embedding task types for different operations:
- `RETRIEVAL_DOCUMENT` — For storing memories and code chunks
- `RETRIEVAL_QUERY` — For searching memories (recall)
- `CODE_RETRIEVAL_QUERY` — For searching code (semantic search)

Key methods: `embed_for_storage()`, `embed_for_query()`, `embed_code()`.

## Note

Pending embedding queue operations (e.g., `PendingEmbedding`, `get_pending_embeddings_sync()`) live in `db/embeddings.rs`, not here.
