---
name: efficiency
description: This skill should be used when the user asks "token efficiency", "how much context", "injection stats", "show efficiency", "token usage", "mira efficiency", "are hooks working", "how much is mira injecting", or wants to see how Mira's token efficiency features are performing.
---

# Mira Efficiency Report

## Injection Stats

!`mira tool session '{"action":"storage_status"}'`

## Injection Feedback

!`mira tool insights '{"action":"insights","insight_source":"injection_feedback"}'`

## Instructions

Present a concise efficiency dashboard from the data above:

### Injection Overview
- **Total injections**: from `injection_total_count` in server_state
- **Total chars injected**: from `injection_total_chars` in server_state
- **Avg chars/injection**: computed from above

### Feedback Quality
- From injection_feedback data: how many injections were referenced vs not
- **Hit rate**: % of injections where the context was actually used by Claude

### Active Efficiency Features
List the active token-saving mechanisms:
- **PreToolUse keyword tightening**: Requires 2+ keywords with AND-join (reduces false recalls)
- **SubagentStart type-aware budgets**: Narrow agents (Explore, code-reviewer) get 800 char cap vs 2000 for full agents
- **Batch-aware cooldown replay**: Parallel tool calls get cached context instead of re-running embeddings
- **File-read cache**: Advises when re-reading unchanged files
- **Cross-prompt injection dedup**: Suppresses identical context on consecutive prompts
- **Post-compaction recovery**: Injects saved decisions/work after context compaction
- **Read symbol hints**: Shows symbol map for large files (>200 lines) from code index

If injection stats are empty or unavailable, note that stats accumulate over time as hooks fire.
