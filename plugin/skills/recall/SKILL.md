---
name: recall
description: This skill should be used when the user asks "show my memories", "what do I have stored", "recall memories about X", "browse memories", "list memories", "what did we decide about", "what did we save", "show stored facts", or wants to search or browse their persistent memory.
argument-hint: "[query]"
---

# Memory Recall

Browse or search persistent memories stored across sessions.

**Query:** $ARGUMENTS

## Instructions

### No arguments — browse recent memories

If no query is provided, list recent memories:

1. Use the `mcp__mira__memory` tool:
   ```
   memory(action="list", limit=20)
   ```
2. Present each memory with:
   - **ID** — needed for forget/archive actions
   - **Content** — the stored fact
   - **Category** and **type** if set
   - **Created date**
3. Group by category if multiple categories are present

### With a query — semantic recall

If a query is provided, search by meaning:

1. Use the `mcp__mira__memory` tool:
   ```
   memory(action="recall", query="<the query>")
   ```
2. Present results with:
   - **ID** and **content**
   - **Relevance** — how closely it matches the query
   - **Category** and **type** if set
3. If no results found, suggest broadening the query or listing all memories

## Follow-Up Hints

After showing results, mention available actions:
- `/mira:remember <content>` — store a new memory
- `memory(action="forget", id=N)` — delete a memory by ID
- `memory(action="archive", id=N)` — archive (hide from auto-export, keep history)

## Example Usage

```
/mira:recall                      # Browse recent memories
/mira:recall authentication       # Find memories about auth
/mira:recall database decisions   # Recall past DB decisions
/mira:recall --category api       # Recall memories in a category
```
