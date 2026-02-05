---
name: remember
description: This skill should be used when the user asks "remember this", "save this decision", "store this for later", "note that we decided", or wants to persist information across sessions.
---

# Quick Memory Storage

Store a memory for future recall across sessions.

**Input:** $ARGUMENTS

## Instructions

1. Parse the input to extract:
   - **Content**: The main text to remember (required)
   - **Category**: Optional, extract from `--category X` or infer from content
   - **Type**: Optional, extract from `--type X` (decision, preference, context, general)

2. Use the `mcp__mira__memory` tool:
   ```
   memory(action="remember", content="...", category="...", fact_type="...")
   ```

3. Confirm storage with the memory ID

## Inference Rules

If no explicit flags provided:
- Content mentions "decided" or "chose" → `fact_type: decision`
- Content mentions "prefer" or "always" or "never" → `fact_type: preference`
- Content describes architecture or patterns → `category: architecture`
- Content describes workflow or process → `category: workflow`

## Examples

```
/mira:remember We use the builder pattern for config structs
→ memory(action="remember", content="We use the builder pattern for config structs", category="patterns", fact_type="decision")

/mira:remember --type preference Peter prefers concise responses
→ memory(action="remember", content="Peter prefers concise responses", fact_type="preference")

/mira:remember --category api The rate limit is 100 req/min
→ memory(action="remember", content="The rate limit is 100 req/min", category="api", fact_type="context")
```
