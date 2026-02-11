# WsEvent

Event types used internally by the MCP router for tool lifecycle broadcasting (`ToolStart`, `ToolResult`) and agent collaboration messaging (`AgentResponse`). Originally designed for WebSocket transport; now used as an internal broadcast mechanism within the server.

**Crate:** `mira-types`
**Source:** `crates/mira-types/src/lib.rs`

## Definition

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsEvent {
    ToolStart {
        tool_name: String,
        arguments: serde_json::Value,
        call_id: String,
    },
    ToolResult {
        tool_name: String,
        result: String,
        success: bool,
        call_id: String,
        duration_ms: u64,
    },
    AgentResponse {
        in_reply_to: String,
        from: AgentRole,
        content: String,
        complete: bool,
    },
}
```

## Variants

### ToolStart

Emitted when an agent begins executing a tool.

| Field | Type | Description |
|-------|------|-------------|
| `tool_name` | `String` | Name of the MCP tool being called |
| `arguments` | `serde_json::Value` | JSON arguments passed to the tool |
| `call_id` | `String` | Unique ID to correlate with the corresponding `ToolResult` |

### ToolResult

Emitted when a tool execution completes.

| Field | Type | Description |
|-------|------|-------------|
| `tool_name` | `String` | Name of the MCP tool that was called |
| `result` | `String` | Output text from the tool |
| `success` | `bool` | Whether the tool call succeeded |
| `call_id` | `String` | Correlates with the originating `ToolStart` |
| `duration_ms` | `u64` | Execution time in milliseconds |

### AgentResponse

A collaboration message from an agent (Mira or Claude).

| Field | Type | Description |
|-------|------|-------------|
| `in_reply_to` | `String` | Message ID this is responding to |
| `from` | `AgentRole` | Which agent sent the message (`mira` or `claude`) |
| `content` | `String` | Response content |
| `complete` | `bool` | Whether the response is complete (false if more info needed) |

## Serialization

Uses internally tagged representation with `"type"` as the discriminator:

```json
{
  "type": "tool_start",
  "tool_name": "recall",
  "arguments": { "query": "auth decisions" },
  "call_id": "call-abc123"
}
```

```json
{
  "type": "tool_result",
  "tool_name": "recall",
  "result": "Found 3 memories...",
  "success": true,
  "call_id": "call-abc123",
  "duration_ms": 45
}
```

```json
{
  "type": "agent_response",
  "in_reply_to": "msg-xyz",
  "from": "claude",
  "content": "The auth system uses JWT tokens.",
  "complete": true
}
```

## See Also

- [AgentRole](AgentRole.md) - Agent identity enum used in `AgentResponse`
- [reply_to_mira](../tools/reply_to_mira.md) - Tool that sends `AgentResponse` events
