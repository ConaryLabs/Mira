<!-- docs/modules/mira-server/background.md -->
# background

Background workers for idle-time processing. Split into two lanes based on latency requirements.

## Lanes

### Fast Lane
Handles embedding generation and incremental indexing. Woken immediately via `FastLaneNotify.wake()` when new work arrives.

### Slow Lane
Handles analysis tasks on a longer polling interval: pondering/insights, code health, documentation gap scanning, diff analysis.

## Sub-modules

| Module | Lane | Purpose |
|--------|------|---------|
| `fast_lane` | fast | Embedding queue processing, incremental indexing |
| `slow_lane` | slow | Task scheduling with priority and circuit breakers |
| `embeddings` | fast | Pending embedding generation |
| `pondering` | slow | Cross-session insight generation (stale goals, fragile modules, revert clusters, recurring errors) |
| `code_health` | slow | Compiler warnings and unused function detection |
| `documentation` | slow | Documentation gap scanning |
| `diff_analysis` | slow | Factual diff stats and call graph impact |
| `watcher` | independent | Filesystem watching for incremental updates |

## Background Task Behavior

All background tasks run using local analysis. No LLM provider is required.

| Task | Method |
|------|--------|
| Diff analysis | Factual stats + call graph impact |
| Pondering/insights | DB queries: stale goals, fragile modules, revert clusters, recurring errors |
| Code health | `cargo check` warnings + call graph unused function detection |

## Key Types

- `FastLaneNotify` - Notification handle to wake the fast lane worker
- `FastLaneWorker` / `SlowLaneWorker` - Worker implementations with supervisor pattern

## Entry Points

- `spawn()` - Spawn both workers with a single pool
- `spawn_with_pools()` - Spawn with separate main/code pools
