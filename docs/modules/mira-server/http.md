# http

Shared HTTP client factory for all network operations. Provides pre-configured reqwest clients with appropriate timeouts.

## Key Functions

- `create_shared_client()` - 5-minute timeout for LLM operations, with connection pooling
- `create_fast_client()` - 30-second timeout for embeddings and quick API calls

## Constants

| Constant | Value | Usage |
|----------|-------|-------|
| `DEFAULT_TIMEOUT` | 300s | LLM inference calls |
| `CONNECT_TIMEOUT` | 30s | TCP connection timeout |
| `FAST_TIMEOUT` | 30s | Embedding and quick API calls |
