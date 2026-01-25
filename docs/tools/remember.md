# remember

Store a fact for future recall with configurable visibility scope.

## Usage

Call the tool with a JSON object containing the memory content and optional metadata. The tool supports upsert behavior when a key is provided.

```json
{
  "name": "remember",
  "arguments": {
    "content": "The user prefers dark mode for code editing",
    "key": "ui_preference_dark_mode",
    "fact_type": "preference",
    "category": "ui",
    "confidence": 0.9,
    "scope": "personal"
  }
}
```

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| content | String | Yes | The factual content to store |
| key | String | No | Unique identifier for upsert operations. If provided and a memory with this key exists, it will be updated instead of creating a new one |
| fact_type | String | No | Type of fact: `preference`, `decision`, `context`, or `general` (default: `general`) |
| category | String | No | Organizational category for grouping related memories |
| confidence | Float | No | Confidence score between 0.0 and 1.0 (default: 1.0) |
| scope | String | No | Visibility scope: `personal` (only creator), `project` (default, anyone with project access), `team` (team members only) |

## Returns

Returns a success message with the memory ID when the fact is stored successfully. If a key was provided and an existing memory was updated, the message indicates the update.

Example output:
```
Memory stored successfully with ID: 12345
```

## Examples

**Store a developer preference:**
```json
{
  "content": "The team uses TypeScript strict mode with noImplicitAny enabled",
  "key": "typescript_config",
  "fact_type": "decision",
  "category": "development",
  "confidence": 0.95,
  "scope": "project"
}
```
*Output:* `Memory stored successfully with ID: 12346`

**Store a personal context note:**
```json
{
  "content": "I'm currently working on the authentication middleware refactor",
  "fact_type": "context",
  "scope": "personal"
}
```
*Output:* `Memory stored successfully with ID: 12347`

**Update an existing memory:**
```json
{
  "content": "The team uses TypeScript strict mode with all strict flags enabled",
  "key": "typescript_config",
  "fact_type": "decision",
  "category": "development",
  "confidence": 0.98,
  "scope": "project"
}
```
*Output:* `Memory updated successfully (ID: 12346)`

## Errors

- **Database connection error**: Returns "Failed to store memory: [error details]" when the database operation fails
- **Invalid confidence value**: Returns an error if confidence is provided but not in the range 0.0-1.0
- **Invalid scope**: Returns an error if scope is provided but not one of: personal, project, team
- **Session not initialized**: Returns an error if no active session exists (session_start or set_project must be called first)

## See Also

- [`recall`](./recall.md): Search memories using semantic similarity
- [`forget`](./forget.md): Delete a memory by ID
- [`session_start`](./session_start.md): Initialize session with project context
- [`set_project`](./set_project.md): Set active project for memory storage