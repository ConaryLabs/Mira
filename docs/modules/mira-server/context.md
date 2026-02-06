# context

Proactive context injection system. Orchestrates multiple injectors to provide relevant context to LLM interactions with budget management and caching.

## Key Type

`ContextInjectionManager` - Orchestrates semantic, file-aware, goal-aware, and convention injectors. Returns `InjectionResult` with context text, sources, and metadata.

## Sub-modules

| Module | Purpose |
|--------|---------|
| `semantic` | Semantic similarity-based context injection |
| `file_aware` | File-aware context injection |
| `goal_aware` | Goal/task-aware context injection |
| `convention` | Convention-aware context injection |
| `working_context` | Working context management |
| `budget` | Token budget management for context windows |
| `cache` | Caching layer for injection results |
| `config` | Injection configuration |
| `analytics` | Context injection analytics and metrics |
