<!-- docs/tools/memory.md -->
# Memory

Persistent cross-session knowledge base. Store and retrieve decisions, preferences, patterns, and context across sessions.

> **MCP actions:** `remember`, `recall`, `list`, `forget`, `archive`, `entities`
> Actions marked (CLI-only) below are available via `mira tool memory '<json>'`.

## Actions

### remember

Store a fact for future recall.

**Parameters:**
- `action` (string, required) - `"remember"`
- `content` (string, required) - The fact to store (max 10KB)
- `key` (string, optional) - Upsert key; if a memory with this key exists, it is replaced
- `fact_type` (string, optional) - One of: `preference`, `decision`, `context`, `general` (default: `general`)
- `category` (string, optional) - Freeform category for organization
- `confidence` (float, optional) - Confidence score 0.0-1.0 (default: 0.8)
- `scope` (string, optional) - Visibility: `personal`, `project` (default), `team`

**Returns:** Memory ID.

**Security:** Content is scanned for secrets (API keys, tokens, passwords). Memories containing secrets are rejected with an error. Content matching prompt injection patterns is flagged as suspicious and excluded from auto-injection and exports.

### recall

Search memories using semantic similarity with keyword fallback.

**Parameters:**
- `action` (string, required) - `"recall"`
- `query` (string, required) - Search query
- `limit` (integer, optional) - Max results, 1-100 (default: 10)
- `category` (string, optional) - Filter by category
- `fact_type` (string, optional) - Filter by fact type

**Returns:** List of matching memories with IDs, content, similarity scores, and fact types.

**Search strategy:** Tries semantic (embedding) search first, falls back to fuzzy search, then SQL LIKE. Results are branch-aware and entity-boosted.

### list

Browse all memories with pagination and optional filtering.

**Parameters:**
- `action` (string, required) - `"list"`
- `limit` (integer, optional) - Max results per page, 1-100 (default: 20)
- `offset` (integer, optional) - Number of results to skip (default: 0)
- `category` (string, optional) - Filter by category
- `fact_type` (string, optional) - Filter by fact type

**Returns:** Paginated list of memories with IDs, content, fact type, category, scope, key, and creation time. Includes total count and whether more pages exist.

### forget

Delete a memory by ID.

**Parameters:**
- `action` (string, required) - `"forget"`
- `id` (integer, required) - Memory ID to delete (must be positive)

**Returns:** Confirmation or "not found" message. Also cleans up orphaned entities.

### archive

Exclude a memory from auto-export to CLAUDE.local.md while keeping it in the database for history and recall.

**Parameters:**
- `action` (string, required) - `"archive"`
- `id` (integer, required) - Memory ID to archive (must be positive)

**Returns:** Confirmation message.

### entities

Query the entity graph for the current project. Entities (technologies, concepts, people, etc.) are automatically extracted from memories and linked to facts.

**Parameters:**
- `action` (string, required) - `"entities"`
- `query` (string, optional) - Filter entities by name (substring match)
- `limit` (integer, optional) - Max results, 1-200 (default: 50)

**Returns:** List of entities with ID, canonical name, entity type, display name, and number of linked memories. Sorted by linked fact count (most referenced first).

### export (CLI-only)

Export all active project memories as structured JSON.

**Parameters:**
- `action` (string, required) - `"export"`

**Returns:** All non-archived, non-suspicious memories with full metadata (ID, content, fact type, category, scope, key, branch, confidence, timestamps). Includes project name and export timestamp.

### purge (CLI-only)

Delete all memories for the current project. Requires explicit confirmation.

**Parameters:**
- `action` (string, required) - `"purge"`
- `confirm` (boolean, required) - Must be `true` to proceed

**Returns:** Count of deleted memories.

**Safety:** Without `confirm=true`, returns an error showing how many memories would be deleted. Also removes associated vector embeddings and orphaned entities.

### export_claude_local (CLI-only)

Export active project memories to CLAUDE.local.md in the project root. Organizes by fact type (Preferences, Decisions, General).

**Parameters:**
- `action` (string, required) - `"export_claude_local"`

**Returns:** Export path and count of exported memories.

## Scoping

| Scope | Visibility | Requirements |
|-------|-----------|-------------|
| `project` | Anyone working on the same project | Default |
| `personal` | Only the creator | Requires user identity |
| `team` | Only members of the active team | Requires active team session |

Access control is enforced on `forget` and `archive` -- you cannot modify memories outside your scope.

## Rate Limiting

A maximum of 50 new memories can be created per session. Key-based upserts that update existing memories do not count toward this limit.

## Examples

```json
{"action": "remember", "content": "Use builder pattern for Config structs", "fact_type": "decision", "category": "patterns"}
```

```json
{"action": "recall", "query": "authentication design", "limit": 5}
```

```json
{"action": "list", "limit": 10, "offset": 20, "category": "patterns"}
```

```json
{"action": "remember", "content": "Always use debug builds", "key": "build_mode", "fact_type": "preference"}
```

```json
{"action": "forget", "id": 42}
```

```json
{"action": "archive", "id": 15}
```

```json
{"action": "entities", "query": "rust"}
```

```json
{"action": "export"}
```

```json
{"action": "purge", "confirm": true}
```

```json
{"action": "export_claude_local"}
```

## Errors

- **"content is required"** - `remember` needs content
- **"query is required"** - `recall` needs a query
- **"id is required"** - `forget` and `archive` need an ID
- **"Content appears to contain a secret"** - Secret detected in content; use `~/.mira/.env` instead
- **"Invalid scope"** - Must be `personal`, `project`, or `team`
- **"Memory content too large"** - Content exceeds 10KB limit
- **"Memory content cannot be empty"** - Content is empty or whitespace-only
- **"Access denied"** - Cannot modify memories from another scope/user/team
- **"Rate limit exceeded"** - Too many memories created in this session (max 50)
- **"Cannot purge: no active project"** - Purge requires an active project
- **"Use confirm=true to delete all N memories"** - Purge requires explicit confirmation

## See Also

- [project](./project.md) - Project context (memories are scoped to projects)
- [session](./session.md) - Recap includes relevant memories
- [team](./team.md) - Team-scoped memories via `scope="team"`
