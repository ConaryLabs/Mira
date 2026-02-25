<!-- docs/modules/mira-server/background/pondering.md -->
# background/pondering

Active reasoning loops that analyze project data during idle time to discover actionable insights.

## Overview

Runs during the slow lane's background cycle, examining recent project activity (tool usage, memories, goals, code health) to generate insights about workflow patterns, potential issues, and improvement opportunities. Each project is pondered at most once every 6 hours. Requires either meaningful project data or at least 10 tool calls before processing.

## Key Functions

- `process_pondering()` - Main entry point. Iterates active projects, gathers data, generates insights via heuristic analysis.
- `cleanup_stale_insights()` - Remove old insights that are no longer relevant

## Sub-modules

| Module | Purpose |
|--------|---------|
| `queries` | Rich data gathering queries (tool history, memories, project signals) |
| `heuristic` | Heuristic insight generation (tool usage stats, friction detection, focus areas) |
| `storage` | Insight persistence and cleanup |
| `types` | Data types for insight context |

## Architecture Notes

Pondering uses a per-project cooldown stored in `server_state`. The cooldown is only advanced after successful storage of insights, so transient DB failures trigger a retry on the next cycle. Insights are stored as behavior patterns via `storage::store_insights()`. The heuristic path generates insights from tool usage statistics, friction detection, and focus area analysis without requiring an LLM.
