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
- `ApiKeys` - API key and host management (DeepSeek, Zhipu, Ollama, OpenAI, Brave)
- `EmbeddingsConfig` - Embedding generation settings
- `MiraConfig` - File-based project configuration
