# memory

Manage persistent memories. Actions: `remember` (store), `recall` (search), `forget` (delete).

## Usage

```json
{
  "name": "memory",
  "arguments": {
    "action": "remember",
    "content": "The team uses builder pattern for config structs",
    "fact_type": "decision",
    "category": "architecture"
  }
}
```

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| action | String | Yes | `remember`, `recall`, or `forget` |
| content | String | For remember | The factual content to store |
| key | String | No | Unique key for upsert (remember) |
| fact_type | String | No | `preference`, `decision`, `context`, or `general` (default: `general`) |
| category | String | No | Organizational category for grouping |
| confidence | Float | No | 0.0–1.0 (default: 1.0) |
| scope | String | No | `personal`, `project` (default), or `team` |
| query | String | For recall | Search query for semantic similarity |
| limit | Integer | No | Max results for recall (default: 10) |
| id | String | For forget | Memory ID to delete |

## Actions

### `remember` — Store a fact

Stores a memory with optional metadata. Supports upsert via `key`.

```json
{
  "name": "memory",
  "arguments": {
    "action": "remember",
    "content": "We use TypeScript strict mode with noImplicitAny",
    "key": "typescript_config",
    "fact_type": "decision",
    "category": "development",
    "scope": "project"
  }
}
```

Returns: `Memory stored successfully with ID: 123` or `Memory updated successfully (ID: 123)`

### `recall` — Search memories

Searches memories using semantic similarity with optional filters.

```json
{
  "name": "memory",
  "arguments": {
    "action": "recall",
    "query": "authentication decisions",
    "category": "architecture",
    "limit": 5
  }
}
```

Returns: JSON array of matching memories with similarity scores.

### `forget` — Delete a memory

Removes a memory from both the SQL database and vector index.

```json
{
  "name": "memory",
  "arguments": {
    "action": "forget",
    "id": "42"
  }
}
```

Returns: `Memory 42 deleted.` or `Memory 42 not found.`

## Errors

- **Invalid action**: Must be `remember`, `recall`, or `forget`
- **Missing content**: `remember` requires `content`
- **Missing query**: `recall` requires `query`
- **Missing id**: `forget` requires `id`
- **Invalid confidence**: Must be 0.0–1.0
- **Invalid scope**: Must be `personal`, `project`, or `team`
- **Secret detection**: Blocks storage of API keys, tokens, and passwords

## See Also

- [**code**](./code.md): Search code by meaning
- [**session**](./session.md): Session recap includes recent memories
- [**project**](./project.md): Initialize project context (imports CLAUDE.local.md)
