<!-- docs/tools/memory.md -->
# Memory

Persistent cross-session knowledge base. Store and retrieve decisions, preferences, patterns, and context across sessions.

> **MCP actions:** `remember`, `recall`, `forget`, `archive`
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

**Security:** Content is scanned for secrets (API keys, tokens, passwords). Memories containing secrets are rejected with an error.

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

### forget

Delete a memory by ID.

**Parameters:**
- `action` (string, required) - `"forget"`
- `id` (integer, required) - Memory ID to delete (must be positive)

**Returns:** Confirmation or "not found" message.

### archive

Exclude a memory from auto-export to CLAUDE.local.md while keeping it in the database.

**Parameters:**
- `action` (string, required) - `"archive"`
- `id` (integer, required) - Memory ID to archive (must be positive)

**Returns:** Confirmation message.

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

## Examples

```json
{"action": "remember", "content": "Use builder pattern for Config structs", "fact_type": "decision", "category": "patterns"}
```

```json
{"action": "recall", "query": "authentication design", "limit": 5}
```

```json
{"action": "remember", "content": "Always use debug builds", "key": "build_mode", "fact_type": "preference"}
```

```json
{"action": "forget", "id": 42}
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
- **"Access denied"** - Cannot modify memories from another scope/user/team

## See Also

- [project](./project.md) - Project context (memories are scoped to projects)
- [session](./session.md) - Recap includes relevant memories
- [team](./team.md) - Team-scoped memories via `scope="team"`
