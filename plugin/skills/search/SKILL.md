---
name: search
description: This skill should be used when the user asks "find code that does X", "search for X", "where is X implemented", "what handles X", "find the X logic", "search by meaning", "semantic search", or needs to find code by concept or intent rather than exact text matching.
argument-hint: "<query>"
---

# Semantic Code Search

Search the codebase using Mira's semantic search to find code by meaning, not just text.

**Query:** $ARGUMENTS

## Instructions

### No arguments -- ask what to search for

If no query is provided ($ARGUMENTS is empty), ask the user what they want to find. Suggest examples:

- "authentication middleware"
- "error handling patterns"
- "database connection logic"

### With a query -- semantic search

If a query is provided:

1. Use the `mcp__mira__code` tool:
   ```
   code(action="search", query="<the query>", limit=10)
   ```
2. Present results clearly with:
   - **File path** and **line numbers** -- linked for navigation
   - **Relevance score** -- how closely the code matches the intent
   - **Code snippet** -- a short preview of the matching code
3. Group related results if they come from the same module
4. Omit results with very low relevance scores

### When results are empty or low-relevance

If semantic search returns no results or only poor matches:

1. Suggest refining the query with more specific terms
2. Try a keyword fallback using Grep for literal string matching
3. Note that semantic search works best with descriptive intent ("how sessions are created") rather than exact identifiers ("session_start")

## Example Output

```
Found 3 results for "session cleanup":

1. src/session/manager.rs:142-158 (score: 0.84)
   fn cleanup_expired_sessions() -- removes sessions older than TTL

2. src/hooks/stop.rs:45-62 (score: 0.71)
   fn on_stop() -- snapshots and archives the current session

3. src/background/tasks.rs:88-95 (score: 0.65)
   fn schedule_cleanup() -- periodic session maintenance task
```

## Follow-Up Hints

After showing results, suggest next steps:
- Read a result file for full context
- `code(action="callers", function_name="...")` -- trace what calls a found function
- `code(action="symbols", file_path="...")` -- explore the structure of a result file
- Refine the query if results do not match the intent

## Example Usage

```
/mira:search authentication middleware
/mira:search error handling patterns
/mira:search database connection pooling
/mira:search how are embeddings generated
```
