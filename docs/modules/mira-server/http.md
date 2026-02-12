<!-- docs/modules/mira-server/http.md -->
# http

Shared HTTP client configuration for all network operations.

## Overview

Provides factory functions for creating `reqwest::Client` instances with appropriate timeout and connection pooling settings. A single shared client is created at startup and passed to all modules that need HTTP access (LLM providers, embedding API).

## Key Functions

- `create_shared_client()` - Default client with 5-minute timeout (for LLM operations)
- `create_fast_client()` - Client with 30-second timeout (for embeddings, quick API calls)

## Constants

| Constant | Value | Purpose |
|----------|-------|---------|
| `DEFAULT_TIMEOUT` | 5 min | LLM request timeout |
| `CONNECT_TIMEOUT` | 30 sec | TCP connection timeout |
| `FAST_TIMEOUT` | 30 sec | Embedding/quick API timeout |

## Architecture Notes

Uses reqwest's built-in connection pooling (`pool_max_idle_per_host: 10`). The shared client should be created once and reused across the application lifetime rather than creating new clients per request.
