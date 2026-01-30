# llm

LLM provider abstraction layer. Provides a unified interface for inference across multiple providers.

## Supported Providers

- **DeepSeek** - Primary provider for expert consultation
- **Gemini** - Alternative provider, also used for embeddings
- **OpenAI-compatible** - Generic client for OpenAI-compatible endpoints

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
| `gemini` | Gemini API client |
| `openai_compat` | OpenAI-compatible client |
| `pricing` | Usage tracking and cost calculation |
| `prompt` | `PromptBuilder` for message construction |
| `types` | `Message`, `Tool`, `FunctionCall`, `ChatResult`, `Usage` |
| `context_budget` | Token estimation and message truncation |
| `http_client` | Shared HTTP infrastructure |
