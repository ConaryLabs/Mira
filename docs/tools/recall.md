# recall

Search memories using semantic similarity.

## Usage

```json
{
  "name": "recall",
  "arguments": {
    "query": "search query text",
    "limit": 10,
    "category": "optional category filter",
    "fact_type": "optional type filter"
  }
}
```

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| query | String | Yes | Search query for semantic similarity matching |
| limit | Integer | No | Maximum number of results to return (default: 10) |
| category | String | No | Filter memories by category (e.g., "preference", "decision", "context") |
| fact_type | String | No | Filter memories by type (e.g., "preference", "decision", "context", "general") |

## Returns

Returns a JSON array of memory objects with the following structure:

```json
[
  {
    "id": "memory_id",
    "content": "Memory content text",
    "fact_type": "memory_type",
    "category": "category_name",
    "confidence": 0.85,
    "created_at": "timestamp",
    "similarity_score": 0.92
  }
]
```

Each memory includes a similarity score (0.0-1.0) indicating how closely it matches the query.

## Examples

**Example 1: Basic semantic search**
```json
{
  "name": "recall",
  "arguments": {
    "query": "authentication middleware implementation",
    "limit": 5
  }
}
```

**Expected output:**
```json
[
  {
    "id": "123",
    "content": "We decided to use JWT tokens for authentication with a 24-hour expiration",
    "fact_type": "decision",
    "category": "authentication",
    "confidence": 0.9,
    "created_at": "2024-01-15T10:30:00Z",
    "similarity_score": 0.89
  },
  {
    "id": "124",
    "content": "User preference: Keep login sessions active for 7 days",
    "fact_type": "preference",
    "category": "authentication",
    "confidence": 0.8,
    "created_at": "2024-01-16T14:20:00Z",
    "similarity_score": 0.76
  }
]
```

**Example 2: Filtered search by category**
```json
{
  "name": "recall",
  "arguments": {
    "query": "database schema decisions",
    "category": "database",
    "limit": 3
  }
}
```

**Example 3: Search with type filter**
```json
{
  "name": "recall",
  "arguments": {
    "query": "user interface preferences",
    "fact_type": "preference",
    "limit": 5
  }
}
```

## Errors

- **"Embeddings client not available"**: The tool requires an embeddings client to generate semantic vectors. Ensure embeddings are configured with a valid API key.
- **"No memories found matching query"**: No memories match the search criteria (query and optional filters).
- **"Database error"**: Failed to access the memory database.
- **"Invalid limit parameter"**: The limit parameter must be a positive integer.
- **"Query too short"**: Search query must be at least 3 characters long.

## See Also

- **remember**: Store a fact for future recall
- **forget**: Delete a memory by ID
- **search_code**: Search code by semantic meaning
- **get_session_recap**: Get session recap including recent memories