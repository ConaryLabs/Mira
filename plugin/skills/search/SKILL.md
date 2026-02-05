---
name: search
description: This skill should be used when the user asks "find code that does X", "search for authentication", "where is X implemented", "look for code related to", or needs semantic code search by concept rather than exact text.
---

# Semantic Code Search

Search the codebase using Mira's semantic search to find code by meaning, not just text.

**Query:** $ARGUMENTS

## Instructions

1. Use the `mcp__mira__code` tool with `action="search"` and the query provided above
2. Set an appropriate limit (default: 10 results)
3. Present results clearly with:
   - File path and line numbers
   - Relevance score
   - Code snippet preview
4. Group related results if they're from the same module
5. Suggest follow-up searches if results seem incomplete

## Example Usage

```
/mira:search authentication middleware
/mira:search error handling patterns
/mira:search database connection pooling
```
