# forget

Delete a memory by ID. Removes the memory from both the SQL database and the vector index.

## Usage

```json
{
  "name": "forget",
  "arguments": {
    "id": "42"
  }
}
```

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| id | String | Yes | Memory ID to delete (must be a positive integer) |

## Returns

- **Found and deleted**: `Memory 42 deleted.`
- **Not found**: `Memory 42 not found.`

## Examples

**Example 1: Delete a specific memory**
```json
{
  "name": "forget",
  "arguments": { "id": "42" }
}
```

**Example 2: Use after recall to remove an outdated memory**

First find the memory with `recall`, note its ID, then delete it:
```json
{
  "name": "forget",
  "arguments": { "id": "15" }
}
```

## Errors

- **"Invalid ID format"**: The ID could not be parsed as an integer.
- **"Invalid memory ID: must be positive"**: The ID must be greater than zero.
- **Database errors**: Failed to delete from the memory tables.

## See Also

- **remember**: Store a fact for future recall
- **recall**: Search memories to find IDs for deletion
