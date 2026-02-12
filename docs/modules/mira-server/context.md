<!-- docs/modules/mira-server/context.md -->
# context

Proactive context injection system for enriching user prompts with relevant information.

## Overview

Orchestrates multiple injectors to automatically provide relevant context (memories, conventions, goals, file info) alongside user messages. The `ContextInjectionManager` evaluates whether a message warrants injection, gathers context from several sources, applies a character budget, caches results, and records analytics. Simple commands, very short/long messages, and non-code-related messages are skipped.

## Key Types

- `ContextInjectionManager` -- Main orchestrator; created with database pool, embeddings client, and fuzzy cache
- `InjectionResult` -- Output containing injected context string, sources list, skip reason, and cache hit flag
- `InjectionSource` -- Enum of context sources: Semantic, FileAware, TaskAware, Convention
- `InjectionConfig` -- Configurable thresholds (enable/disable each source, budget, sample rate, message length bounds)
- `BudgetManager` -- Manages total character budget across injectors

## Sub-modules

| Module | Purpose |
|--------|---------|
| `semantic` | Semantic similarity-based context injection (via embeddings or fuzzy) |
| `file_aware` | Extracts file path mentions from messages and injects related context |
| `goal_aware` | Injects active goal context for task-aware responses |
| `convention` | Injects module-specific coding conventions from the database |
| `working_context` | Working context management |
| `budget` | Character budget management across injectors |
| `cache` | TTL-based caching of injection results |
| `config` | Injection configuration (persisted to database) |
| `analytics` | Injection event recording and summary reporting |
