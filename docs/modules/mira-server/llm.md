# llm

LLM provider abstraction layer. Provides a unified interface for inference across multiple providers.

## Supported Providers

- **DeepSeek** — Primary provider for expert consultation
- **Zhipu** — Alternative provider (GLM-4.7)
- **Ollama** — Local LLM via Ollama (background tasks, no API key needed)
- **MCP Sampling** — Routes through the host client

**Note:** The `openai_compat` sub-module is a shared request/response format used internally by DeepSeek, Zhipu, and Ollama. It is not a separately selectable provider.

## Key Types

| Type | Purpose |
|------|---------|
| `LlmClient` | Trait defining the unified provider interface |
| `Provider` | Enum of available providers |
| `ProviderFactory` | Instantiates provider clients from configuration |
| `Message` | Chat message (role + content) |
| `ChatResult` | LLM response with usage stats |
| `NormalizedUsage` | Standardized token usage across providers |
| `PromptBuilder` | Fluent API for constructing message sequences |

## Sub-modules

| Module | Purpose |
|--------|---------|
| `provider` | `LlmClient` trait and `Provider` enum |
| `factory` | `ProviderFactory` for client instantiation |
| `deepseek` | DeepSeek API client |
| `zhipu` | Zhipu GLM-4.7 API client |
| `ollama` | Ollama API client (local LLM, OpenAI-compatible) |
| `sampling` | MCP Sampling-based provider (routes through host client) |
| `openai_compat` | Shared OpenAI-compatible request/response format |
| `pricing` | Usage tracking and cost calculation |
| `logging` | LLM call logging |
| `prompt` | `PromptBuilder` for message construction |
| `types` | `Message`, `Tool`, `FunctionCall`, `ChatResult`, `Usage` |
| `context_budget` | Token estimation and message truncation |
| `http_client` | Shared HTTP infrastructure |
