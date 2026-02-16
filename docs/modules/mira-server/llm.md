<!-- docs/modules/mira-server/llm.md -->
# llm

LLM provider abstraction layer for background inference tasks.

## Overview

Provides a unified interface for chat completion across multiple providers. Used for background tasks like summaries, briefings, pondering, and code health analysis. Not used for primary Claude Code interactions (those go through MCP sampling). The `ProviderFactory` manages client instantiation, provider selection with fallback chains, and circuit breaking for unreliable providers.

## Supported Providers

- **DeepSeek** -- Primary provider for background LLM tasks (deepseek-reasoner)
- **Ollama** -- Local LLM via Ollama (no API key needed)
- **MCP Sampling** -- Fallback provider routing through the host client (Claude Code)

## Key Types

| Type | Purpose |
|------|---------|
| `LlmClient` | Trait defining the unified provider interface (chat, stateful chat, context budget, supports_stateful, provider_type, model_name, normalize_usage) |
| `Provider` | Enum of available providers with parsing, display, and metadata |
| `ProviderFactory` | Creates and manages provider clients with fallback chains and circuit breaking |
| `Message` | Chat message (role + content) |
| `ChatResult` | LLM response with content, tool calls, usage stats |
| `ToolCall` | Tool call request from LLM |
| `FunctionDef` | Function definition for tool use |
| `NormalizedUsage` | Standardized token usage across providers |
| `PromptBuilder` | Fluent API for constructing message sequences |
| `CircuitBreaker` | Tracks provider failures and temporarily disables unhealthy providers |

## Sub-modules

| Module | Purpose |
|--------|---------|
| `provider` | `LlmClient` trait and `Provider` enum |
| `factory` | `ProviderFactory` for client instantiation with fallback and circuit breaking |
| `deepseek` | DeepSeek API client |
| `ollama` | Ollama API client (local LLM, OpenAI-compatible) |
| `sampling` | MCP Sampling-based provider (routes through host client) |
| `openai_compat` | Shared OpenAI-compatible request/response format |
| `pricing` | Usage tracking and cost calculation |
| `logging` | LLM call logging |
| `prompt` | `PromptBuilder` for message construction |
| `types` | `Message`, `Tool`, `FunctionCall`, `ChatResult`, `Usage` |
| `context_budget` | Token estimation and message truncation for context windows |
| `circuit_breaker` | Provider health tracking with automatic recovery |
| `http_client` | Shared HTTP client configuration |

## Architecture Notes

The `openai_compat` sub-module is a shared request/response format used internally by DeepSeek and Ollama -- it is not a separately selectable provider. Provider selection follows a priority chain: configured background provider > default provider > fallback order (DeepSeek > Ollama). Config file (`~/.mira/config.toml`) takes precedence over the `DEFAULT_LLM_PROVIDER` env var.
