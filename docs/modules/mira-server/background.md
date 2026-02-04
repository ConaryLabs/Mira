# background

Background workers for idle-time processing. Split into two lanes based on latency requirements.

## Lanes

### Fast Lane
Handles embedding generation and incremental indexing. Woken immediately via `FastLaneNotify.wake()` when new work arrives (e.g., after storing a memory).

### Slow Lane
Handles LLM-powered tasks: session summaries, pondering/insights, code health analysis, and proactive suggestions. Runs on a longer polling interval.

## Sub-modules

| Module | Lane | Purpose |
|--------|------|---------|
| `fast_lane` | fast | Embedding queue processing, incremental indexing |
| `slow_lane` | slow | LLM task orchestration |
| `embeddings` | fast | Pending embedding generation |
| `session_summaries` | slow | Session summary generation |
| `pondering` | slow | Insight extraction and pattern detection |
| `code_health` | slow | Code health analysis |
| `documentation` | slow | Documentation gap scanning |
| `diff_analysis` | slow | Semantic diff analysis |
| `proactive` | slow | Proactive analysis and suggestions |
| `briefings` | slow | What's-new briefing generation |
| `change_patterns` | slow | Change pattern mining from git history |
| `entity_extraction` | slow | Entity extraction from memories and code |
| `outcome_scanner` | slow | Outcome tracking for predictions |
| `summaries` | slow | Module summary generation |
| `watcher` | fast | Filesystem watching for incremental updates |

## Graceful Degradation

All background tasks degrade gracefully when no LLM provider is configured (or `MIRA_DISABLE_LLM=1`):

| Task | With LLM | Without LLM |
|------|----------|-------------|
| Module summaries | LLM-generated descriptions | Heuristic: file count, languages, top symbols |
| Diff analysis | Semantic change classification | Heuristic: regex-based function/security detection |
| Pondering/insights | LLM-powered pattern extraction | Heuristic: tool usage stats, friction detection, focus areas |
| Session summaries | LLM summarization | Skipped |
| Code health | LLM analysis | Skipped |

Heuristic results are tagged with `[heuristic]` prefix and remain upgradeable when an LLM becomes available.

## Entry Points

- `spawn()` - Spawn both workers with a single pool
- `spawn_with_pools()` - Spawn with separate main/code pools
