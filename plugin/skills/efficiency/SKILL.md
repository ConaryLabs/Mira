---
name: efficiency
description: This skill should be used when the user asks "token efficiency", "how much context", "injection stats", "show efficiency", "token usage", "mira efficiency", "are hooks working", "how much is mira injecting", or wants to see how Mira's token efficiency features are performing.
---

# Mira Efficiency Report

## Storage Stats

!`mira tool session '{"action":"storage_status"}'`

## Instructions

Present a concise efficiency dashboard from the data above:

### Storage Overview
- **Database sizes**: from storage_status
- **Memory count**: total stored memories
- **Session count**: total recorded sessions

Note: Detailed injection stats (total injections, chars injected, hit rate) will be available in a future release.

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
