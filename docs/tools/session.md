<!-- docs/tools/session.md -->
# Session

Session management, analytics, and background task tracking.

> **MCP actions:** `current_session`, `recap`
> All other actions below are **CLI-only** -- use `mira tool session '<json>'`.

## Actions

### current_session

Show the current session ID.

**Parameters:**
- `action` (string, required) - `"current_session"`

**Returns:** Current session ID or "No active session".

### recap

Get a formatted session recap with preferences, recent context, active goals, pending tasks, and Claude Code session notes.

**Parameters:**
- `action` (string, required) - `"recap"`

**Returns:** Formatted text combining preferences, recent memories, and session notes.

### list_sessions (CLI-only)

List recent sessions for the active project.

**Parameters:**
- `action` (string, required) - `"list_sessions"`
- `limit` (integer, optional) - Max results (default: 20)

**Returns:** Session list with IDs, timestamps, status, summaries, and source info (startup vs resume).

### get_history (CLI-only)

View tool call history for a session.

**Parameters:**
- `action` (string, required) - `"get_history"`
- `session_id` (string, optional) - Session ID to query (defaults to current session)
- `limit` (integer, optional) - Max results (default: 20)

**Returns:** Tool calls with names, timestamps, success status, and result previews.

### usage_summary (CLI-only)

Get aggregate LLM usage totals (requests, tokens, cost, average duration).

**Parameters:**
- `action` (string, required) - `"usage_summary"`
- `since_days` (integer, optional) - Look back period in days (default: 30)

**Returns:** Formatted summary with total requests, tokens, estimated cost, and average duration.

### usage_stats (CLI-only)

Get LLM usage statistics grouped by a dimension.

**Parameters:**
- `action` (string, required) - `"usage_stats"`
- `group_by` (string, optional) - Grouping dimension: `role`, `provider`, `model`, or `provider_model` (default: `role`)
- `since_days` (integer, optional) - Look back period in days (default: 30)

**Returns:** Table of usage per group with requests, tokens, and cost.

### usage_list (CLI-only)

List recent LLM usage records grouped by role.

**Parameters:**
- `action` (string, required) - `"usage_list"`
- `since_days` (integer, optional) - Look back period in days (default: 30)
- `limit` (integer, optional) - Max results (default: 50)

**Returns:** List of usage records per role.

### insights (CLI-only)

Query unified insights digest (pondering, proactive, doc gaps).

> **Note:** Also available as a standalone MCP tool. See [insights](./insights.md).

**Parameters:**
- `action` (string, required) - `"insights"`
- `insight_source` (string, optional) - Filter by source: `pondering`, `proactive`, `doc_gap`
- `min_confidence` (float, optional) - Minimum confidence threshold (0.0-1.0, default: 0.5)
- `since_days` (integer, optional) - Look back period in days (default: 30)
- `limit` (integer, optional) - Max results

**Returns:** Insights digest with source, confidence, and content.

### dismiss_insight (CLI-only)

Dismiss an insight by ID.

**Parameters:**
- `action` (string, required) - `"dismiss_insight"`
- `insight_id` (integer, required) - Insight row ID to dismiss
- `insight_source` (string, required) - Source type: `pondering` or `doc_gap`

**Returns:** Confirmation or "not found" message.

### error_patterns (CLI-only)

Show learned error patterns and fixes for the active project.

**Parameters:**
- `action` (string, required) - `"error_patterns"`
- `limit` (integer, optional) - Max results (default: 20)

**Returns:** Error patterns with tool name, fingerprint, occurrence count, fix description, and last seen timestamp. Requires an active project.

### session_lineage (CLI-only)

Show session history with resume chains for the active project.

**Parameters:**
- `action` (string, required) - `"session_lineage"`
- `limit` (integer, optional) - Max results (default: 20)

**Returns:** Sessions with source (startup/resume), resumed_from links, branch, timestamps, and goal counts. Indented formatting shows resume chains.

### capabilities (CLI-only)

Show capability status: what features are available, degraded, or unavailable.

**Parameters:**
- `action` (string, required) - `"capabilities"`

**Returns:** Status of each capability (semantic_search, background_analysis, fuzzy_search, code_index, mcp_sampling) with availability and detail messages.

### report (CLI-only)

Get session injection efficiency report.

**Parameters:**
- `action` (string, required) - `"report"`
- `session_id` (string, optional) - Session ID for session-specific report (omit for cumulative)

**Returns:** Injection statistics: total injections, chars injected, deduped/cached counts, average latency, and efficiency ratio.

### storage_status (CLI-only)

Show database storage size and data retention policy.

**Parameters:**
- `action` (string, required) - `"storage_status"`

**Returns:** Database file sizes, row counts per table, and configured retention periods.

### cleanup (CLI-only)

Run data cleanup to remove old records based on retention policy.

**Parameters:**
- `action` (string, required) - `"cleanup"`
- `category` (string, optional) - Category to clean: `sessions`, `analytics`, `behavior`, `all` (default: `all`)
- `dry_run` (boolean, optional) - Preview what would be cleaned without deleting (default: `true`)

**Returns:** Summary of rows that would be (or were) deleted per table.

## Examples

```json
{"action": "recap"}
```

```json
{"action": "list_sessions", "limit": 5}
```

```json
{"action": "get_history", "session_id": "a1b2c3d4-..."}
```

```json
{"action": "usage_stats", "group_by": "provider_model", "since_days": 7}
```

```json
{"action": "error_patterns", "limit": 10}
```

```json
{"action": "session_lineage"}
```

```json
{"action": "capabilities"}
```

```json
{"action": "report", "session_id": "abc-123"}
```

## Errors

- **"No active project"** - `list_sessions`, `error_patterns`, `session_lineage` require an active project
- **"No active session"** - `get_history` with no session_id and no active session

## See Also

- [project](./project.md) - Initialize session with project context
- [memory](./memory.md) - Memories shown in recap
- [insights](./insights.md) - Background analysis digest (standalone MCP tool)
- [tasks](./tasks.md) - Background task management (separate tool)
