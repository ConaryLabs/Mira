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
| `watcher` | fast | Filesystem watching for incremental updates |

## Entry Points

- `spawn()` - Spawn both workers with a single pool
- `spawn_with_pools()` - Spawn with separate main/code pools
