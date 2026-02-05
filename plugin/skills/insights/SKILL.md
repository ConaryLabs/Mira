---
name: insights
description: This skill should be used when the user asks "what did you learn", "show insights", "any patterns noticed", "what should I know", or wants to see Mira's background analysis and predictions.
---

# Mira Insights

## Current Insights (Live)

!`mira tool session '{"action":"insights","min_confidence":0.5}'`

## Instructions

Present the insights above grouped by source:
- **Pondering**: Background analysis of your work patterns
- **Proactive**: Suggestions based on current context
- **Documentation Gaps**: Missing docs that should be written

For each insight show:
- The insight content
- Confidence score (if above threshold)
- When it was generated

If no insights exist, suggest actions that generate them:
- Working on code (triggers pattern detection)
- Making decisions (triggers pondering analysis)
- Using tools consistently (triggers workflow insights)

## Optional Filters

If user passes arguments, use `mcp__mira__session` tool with filters:
- `--source pondering|proactive|doc_gap` → `insight_source` parameter
- `--min-confidence 0.7` → `min_confidence` parameter
