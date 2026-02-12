<!-- docs/modules/mira-server/proactive/background.md -->
# proactive/background

Background processing for proactive suggestion generation. Mines behavioral patterns and pre-generates contextual suggestions on a tiered schedule.

## Overview

Operates on a two-tier frequency within the slow lane's background cycle:
- **Pattern mining** (every 3rd cycle, ~15 min): SQL-only analysis of behavior logs to detect recurring patterns (file sequences, tool chains, query patterns)
- **Suggestion enhancement** (every 10th cycle, ~50 min): LLM-powered generation of contextual suggestions from high-confidence patterns, or template-based fallback when no LLM is available

## Key Functions

- `process_proactive()` - Main entry point, dispatches based on cycle count
- `get_pre_generated_suggestions()` - Retrieve suggestions for a given trigger
- `mark_suggestion_shown()` / `mark_suggestion_accepted()` - Track suggestion lifecycle
- `cleanup_expired_suggestions()` - Remove stale suggestions

## Sub-modules

| Module | Purpose |
|--------|---------|
| `mining` | Pattern extraction from behavior logs (SQL only) |
| `llm` | LLM-powered suggestion generation and parsing |
| `templates` | Template-based suggestion fallback (no LLM needed) |
| `storage` | Suggestion persistence and cleanup |
| `lookup` | Suggestion retrieval and lifecycle tracking |

## Architecture Notes

Template suggestions use a confidence multiplier (0.85x) relative to LLM-generated suggestions. Patterns must have at least 0.7 confidence to generate template suggestions. Suggestions are stored with trigger keys that match against user context during the `UserPromptSubmit` hook.
