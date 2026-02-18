<!-- docs/modules/mira-server/config.md -->
# config

Configuration management from environment variables and config files.

## Overview

Centralizes all configuration into two sources: environment variables (`EnvConfig`) and a TOML file at `~/.mira/config.toml` (`MiraConfig`). Environment variables are loaded once at startup via `EnvConfig::load()`. The config file is loaded via `MiraConfig::load()` and takes precedence over env vars for LLM provider selection.

## Key Types

- `EnvConfig` -- All environment-based configuration in one struct (API keys, embeddings config, default provider, user ID, fuzzy fallback toggle)
- `ApiKeys` -- API key management with availability checks and redacted debug output
- `EmbeddingsConfig` -- Embedding dimensions configuration (provider-dependent; 1536 for OpenAI, provider-reported for Ollama)
- `MiraConfig` -- File-based config from `~/.mira/config.toml` (LLM provider preferences)
- `ConfigValidation` -- Validation results with warnings and errors

## Sub-modules

| Module | Purpose |
|--------|---------|
| `env` | Environment-based configuration (`EnvConfig`, `ApiKeys`, `EmbeddingsConfig`, `ConfigValidation`) |
| `file` | File-based configuration (`MiraConfig` from `~/.mira/config.toml`) |
| `ignore` | Directory ignore lists for indexing (common, Python, Node, Go specific skip patterns, `.miraignore` support) |

## Architecture Notes

`ApiKeys` provides capability checks (`has_llm_provider()`, `has_embeddings()`, `has_web_search()`) used throughout the server to gate features. The `ignore` sub-module provides centralized skip lists used by the cartographer and indexer to avoid scanning build artifacts, virtual environments, and other non-source directories. Projects can add custom patterns via a `.miraignore` file.
