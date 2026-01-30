# config

Configuration and shared constants management.

## Sub-modules

| Module | Purpose |
|--------|---------|
| `env` | Environment-based configuration (`EnvConfig`, `ApiKeys`, `EmbeddingsConfig`) |
| `file` | File-based configuration (`MiraConfig`) |
| `ignore` | Gitignore-style pattern matching for file filtering |

## Key Types

- `EnvConfig` - Configuration loaded from environment variables
- `ApiKeys` - API key management (DeepSeek, Gemini)
- `EmbeddingsConfig` - Embedding generation settings
- `MiraConfig` - File-based project configuration
