# Mira API Reference

This document describes the Mira backend API, including WebSocket and HTTP endpoints.

## Table of Contents

1. [Overview](#overview)
2. [Authentication](#authentication)
3. [WebSocket API](#websocket-api)
4. [HTTP API](#http-api)
5. [Message Types](#message-types)
6. [Operation Events](#operation-events)
7. [Built-in Commands](#built-in-commands)
8. [Error Handling](#error-handling)

---

## Overview

Mira uses a hybrid API architecture:

- **WebSocket** (`/ws`): Real-time communication for chat, streaming, and operations
- **HTTP** (`/api/*`): Authentication and stateless requests

### Base URLs

| Environment | WebSocket | HTTP |
|-------------|-----------|------|
| Development | `ws://localhost:3001/ws` | `http://localhost:3001/api` |
| Production | `wss://yourdomain.com/ws` | `https://yourdomain.com/api` |

### Ports

- Backend: 3001
- Qdrant HTTP: 6333
- Qdrant gRPC: 6334

---

## Authentication

### POST /api/login

Authenticate a user and receive a JWT token.

**Request:**
```json
{
  "username": "string",
  "password": "string"
}
```

**Response (200):**
```json
{
  "token": "eyJ...",
  "user_id": "user-123",
  "username": "johndoe"
}
```

**Response (401):**
```json
{
  "error": "Invalid credentials"
}
```

### POST /api/register

Create a new user account.

**Request:**
```json
{
  "username": "string",
  "password": "string",
  "email": "string (optional)"
}
```

**Response (200):**
```json
{
  "token": "eyJ...",
  "user_id": "user-456",
  "username": "newuser"
}
```

**Response (400):**
```json
{
  "error": "Username already exists"
}
```

### POST /api/verify

Verify a JWT token is valid.

**Request:**
```json
{
  "token": "eyJ..."
}
```

**Response:**
```json
{
  "valid": true,
  "user_id": "user-123",
  "username": "johndoe"
}
```

---

## WebSocket API

### Connection

Connect to the WebSocket endpoint:

```javascript
const ws = new WebSocket('ws://localhost:3001/ws');

ws.onopen = () => {
  console.log('Connected');
};

ws.onmessage = (event) => {
  const message = JSON.parse(event.data);
  console.log('Received:', message);
};
```

### Message Format

All messages are JSON with a `type` field that determines the message structure.

---

## Message Types

### Client Messages (Frontend to Backend)

#### Chat Message

Send a chat message to the assistant.

```json
{
  "type": "chat",
  "content": "Explain this code to me",
  "project_id": "proj-123",
  "metadata": {
    "file_path": "/src/main.rs",
    "file_content": "fn main() { ... }",
    "language": "rust",
    "repo_id": "repo-456",
    "project_name": "my-project",
    "has_repository": true,
    "repo_root": "/home/user/my-project",
    "branch": "main",
    "selection": {
      "start_line": 10,
      "end_line": 20,
      "text": "selected code..."
    }
  }
}
```

#### Command Message

Execute a built-in or custom command.

```json
{
  "type": "command",
  "command": "reload-commands",
  "args": {}
}
```

#### Project Command

Execute a project-related operation.

```json
{
  "type": "project_command",
  "method": "import",
  "params": {
    "path": "/path/to/project"
  }
}
```

Available methods:
- `import`: Import a project from path
- `list`: List all projects
- `get`: Get project details
- `delete`: Delete a project
- `analyze`: Trigger code analysis

#### Memory Command

Execute a memory-related operation.

```json
{
  "type": "memory_command",
  "method": "search",
  "params": {
    "query": "authentication code",
    "limit": 10
  }
}
```

Available methods:
- `search`: Semantic search in memory
- `get_recent`: Get recent messages
- `get_summary`: Get session summary
- `clear`: Clear session memory

#### Git Command

Execute a Git-related operation.

```json
{
  "type": "git_command",
  "method": "status",
  "params": {
    "project_id": "proj-123"
  }
}
```

Available methods:
- `status`: Get git status
- `diff`: Get file diff
- `log`: Get commit history
- `branches`: List branches
- `checkout`: Checkout branch

#### FileSystem Command

Execute filesystem operations.

```json
{
  "type": "file_system_command",
  "method": "read",
  "params": {
    "path": "/src/main.rs"
  }
}
```

Available methods:
- `read`: Read file contents
- `write`: Write to file
- `list`: List directory
- `delete`: Delete file
- `mkdir`: Create directory

#### Code Intelligence Command

Execute code analysis operations.

```json
{
  "type": "code_intelligence_command",
  "method": "search_semantic",
  "params": {
    "query": "authentication",
    "project_id": "proj-123",
    "limit": 20
  }
}
```

Available methods:
- `search_semantic`: Semantic code search
- `get_call_graph`: Get function call graph
- `get_patterns`: Get detected design patterns
- `get_cochange`: Get co-change suggestions

#### Typing Indicator

Signal typing state to other clients.

```json
{
  "type": "typing",
  "active": true
}
```

---

### Server Messages (Backend to Frontend)

#### Connection Ready

Sent when WebSocket connection is established.

```json
{
  "type": "connection_ready"
}
```

#### Stream (Token Delta)

Real-time streaming of LLM response tokens.

```json
{
  "type": "stream",
  "delta": "The"
}
```

#### Chat Complete

Final response with full content and artifacts.

```json
{
  "type": "chat_complete",
  "user_message_id": "msg-123",
  "assistant_message_id": "msg-456",
  "content": "Here is the explanation...",
  "artifacts": [
    {
      "id": "artifact-789",
      "path": "/src/example.rs",
      "content": "fn example() { ... }",
      "language": "rust",
      "kind": "code",
      "diff": "@@ -1,3 +1,5 @@...",
      "is_new_file": false
    }
  ],
  "thinking": "Let me analyze this step by step..."
}
```

#### Status Update

General status message for UI updates.

```json
{
  "type": "status",
  "message": "Analyzing code...",
  "detail": "Processing 15 files"
}
```

#### Error

Error message with code.

```json
{
  "type": "error",
  "message": "Failed to read file",
  "code": "FILE_NOT_FOUND"
}
```

#### Data Response

Generic data response with optional request ID for matching.

```json
{
  "type": "data",
  "data": { ... },
  "request_id": "req-123"
}
```

#### Image Generated

Result of image generation tool.

```json
{
  "type": "image_generated",
  "urls": ["https://..."],
  "revised_prompt": "A detailed image of..."
}
```

#### Pong

Heartbeat response.

```json
{
  "type": "pong"
}
```

---

## Operation Events

The Operation Engine emits events during complex multi-step operations.

### operation.started

Operation has begun.

```json
{
  "type": "operation.started",
  "operation_id": "op-123",
  "timestamp": 1701619200
}
```

### operation.streaming

Streaming content from LLM.

```json
{
  "type": "operation.streaming",
  "operation_id": "op-123",
  "content": "Here is ",
  "timestamp": 1701619201
}
```

### operation.plan_generated

Thinking/planning phase complete.

```json
{
  "type": "operation.plan_generated",
  "operation_id": "op-123",
  "plan_text": "1. First, I will analyze...",
  "reasoning_tokens": 500,
  "timestamp": 1701619202
}
```

### operation.delegated

Operation delegated to a tool.

```json
{
  "type": "operation.delegated",
  "operation_id": "op-123",
  "delegated_to": "read_file",
  "reason": "Need to read source file",
  "timestamp": 1701619203
}
```

### operation.tool_executed

Tool execution complete.

```json
{
  "type": "operation.tool_executed",
  "operation_id": "op-123",
  "tool_name": "read_file",
  "tool_type": "file",
  "summary": "Read 150 lines from src/main.rs",
  "success": true,
  "details": { ... },
  "timestamp": 1701619204
}
```

### operation.artifact_preview

Preview of artifact being created.

```json
{
  "type": "operation.artifact_preview",
  "operation_id": "op-123",
  "artifact_id": "artifact-456",
  "path": "/src/new_feature.rs",
  "preview": "fn new_feature() {\n    // ...\n}",
  "timestamp": 1701619205
}
```

### operation.artifact_completed

Artifact creation complete.

```json
{
  "type": "operation.artifact_completed",
  "operation_id": "op-123",
  "artifact": {
    "id": "artifact-456",
    "path": "/src/new_feature.rs",
    "content": "fn new_feature() { ... }",
    "language": "rust",
    "kind": "code",
    "diff": "@@ ...",
    "is_new_file": true
  },
  "timestamp": 1701619206
}
```

### operation.task_created

Subtask created within operation.

```json
{
  "type": "operation.task_created",
  "operation_id": "op-123",
  "task_id": "task-789",
  "sequence": 1,
  "description": "Read source files",
  "active_form": "Reading source files",
  "timestamp": 1701619207
}
```

### operation.task_started

Subtask execution started.

```json
{
  "type": "operation.task_started",
  "operation_id": "op-123",
  "task_id": "task-789",
  "timestamp": 1701619208
}
```

### operation.task_completed

Subtask completed successfully.

```json
{
  "type": "operation.task_completed",
  "operation_id": "op-123",
  "task_id": "task-789",
  "timestamp": 1701619209
}
```

### operation.task_failed

Subtask failed.

```json
{
  "type": "operation.task_failed",
  "operation_id": "op-123",
  "task_id": "task-789",
  "error": "File not found",
  "timestamp": 1701619210
}
```

### operation.status_changed

Operation status changed.

```json
{
  "type": "operation.status_changed",
  "operation_id": "op-123",
  "old_status": "generating",
  "new_status": "completed",
  "timestamp": 1701619211
}
```

### operation.completed

Operation completed successfully.

```json
{
  "type": "operation.completed",
  "operation_id": "op-123",
  "result": "Task completed successfully. Created 3 files.",
  "artifacts": [ ... ],
  "timestamp": 1701619212
}
```

### operation.failed

Operation failed with error.

```json
{
  "type": "operation.failed",
  "operation_id": "op-123",
  "error": "API rate limit exceeded",
  "timestamp": 1701619213
}
```

### operation.sudo_approval_required

Dangerous operation requires user approval.

```json
{
  "type": "operation.sudo_approval_required",
  "operation_id": "op-123",
  "approval_request_id": "approval-456",
  "command": "rm -rf /important/directory",
  "reason": "Destructive operation detected",
  "timestamp": 1701619214
}
```

### operation.sudo_approved

Sudo operation approved by user.

```json
{
  "type": "operation.sudo_approved",
  "operation_id": "op-123",
  "approval_request_id": "approval-456",
  "approved_by": "user-789",
  "timestamp": 1701619215
}
```

### operation.sudo_denied

Sudo operation denied by user.

```json
{
  "type": "operation.sudo_denied",
  "operation_id": "op-123",
  "approval_request_id": "approval-456",
  "denied_by": "user-789",
  "reason": "Too risky",
  "timestamp": 1701619216
}
```

---

## Built-in Commands

These commands are processed directly by the backend.

### /commands

List all available slash commands.

```
/commands
```

### /reload-commands

Reload custom commands from disk.

```
/reload-commands
```

### /checkpoints

List available checkpoints for current session.

```
/checkpoints
```

### /rewind <id>

Restore files to a previous checkpoint state.

```
/rewind abc123
```

### /mcp

List connected MCP servers and their tools.

```
/mcp
```

---

## Error Handling

### Error Codes

| Code | Description |
|------|-------------|
| `AUTH_REQUIRED` | Authentication required |
| `INVALID_TOKEN` | JWT token is invalid or expired |
| `FILE_NOT_FOUND` | Requested file does not exist |
| `PERMISSION_DENIED` | User lacks permission for operation |
| `RATE_LIMIT` | API rate limit exceeded |
| `BUDGET_EXCEEDED` | Daily/monthly budget limit reached |
| `QDRANT_ERROR` | Vector database error |
| `LLM_ERROR` | LLM API error |
| `INTERNAL_ERROR` | Internal server error |

### Error Response Format

```json
{
  "type": "error",
  "message": "Human-readable error message",
  "code": "ERROR_CODE"
}
```

---

## Rate Limits

| Resource | Limit | Period |
|----------|-------|--------|
| Chat messages | 60 | per minute |
| API requests | 100 | per minute |
| File operations | 200 | per minute |
| LLM calls | Varies | per budget |

---

## Pagination

For endpoints returning lists, use standard pagination:

```json
{
  "offset": 0,
  "limit": 20
}
```

Response includes total count:

```json
{
  "items": [ ... ],
  "total": 150,
  "offset": 0,
  "limit": 20
}
```

---

## WebSocket Heartbeat

To keep the connection alive, send periodic ping messages:

```javascript
setInterval(() => {
  ws.send(JSON.stringify({ type: "ping" }));
}, 30000);
```

The server responds with:

```json
{
  "type": "pong"
}
```

---

## Example: Complete Chat Flow

```javascript
const ws = new WebSocket('ws://localhost:3001/ws');

// Handle connection
ws.onopen = () => {
  console.log('Connected');
};

// Send chat message
ws.send(JSON.stringify({
  type: 'chat',
  content: 'Write a function to calculate fibonacci numbers',
  project_id: 'proj-123'
}));

// Handle streaming response
ws.onmessage = (event) => {
  const msg = JSON.parse(event.data);

  switch (msg.type) {
    case 'operation.started':
      console.log('Operation started:', msg.operation_id);
      break;

    case 'operation.streaming':
      process.stdout.write(msg.content);
      break;

    case 'operation.artifact_completed':
      console.log('Artifact:', msg.artifact.path);
      break;

    case 'operation.completed':
      console.log('Done:', msg.result);
      break;

    case 'operation.failed':
      console.error('Error:', msg.error);
      break;
  }
};
```

---

## Version History

| Version | Date | Changes |
|---------|------|---------|
| 0.9.0 | 2025-12 | Initial API documentation |
