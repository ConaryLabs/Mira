# reply_to_mira

Send a response back to Mira during agent collaboration. Used when Mira sends a question or task to Claude through the MCP connection and expects a reply.

## Usage

```json
{
  "name": "reply_to_mira",
  "arguments": {
    "in_reply_to": "msg-abc123",
    "content": "The authentication module uses JWT tokens with 24h expiry.",
    "complete": true
  }
}
```

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| in_reply_to | String | Yes | The message_id you are replying to |
| content | String | Yes | Your response content |
| complete | Boolean | No | Whether your response is complete (default: true). Set to false if you need more information. |

## Returns

- **Connected**: `Response sent to Mira`
- **No pending request**: `(Reply not sent - no pending request) Content: {content}`

## Examples

**Example 1: Complete reply to a question**
```json
{
  "name": "reply_to_mira",
  "arguments": {
    "in_reply_to": "msg-abc123",
    "content": "The function handles three edge cases: null input, empty arrays, and duplicate keys."
  }
}
```

**Example 2: Partial reply requesting more info**
```json
{
  "name": "reply_to_mira",
  "arguments": {
    "in_reply_to": "msg-abc123",
    "content": "I need to see the error log to diagnose this. Can you share the stack trace?",
    "complete": false
  }
}
```

## Errors

- **"No pending request found for message_id: {id}"**: The request may have timed out or already been answered.

## See Also

- [**session**](./session.md): View session history and recap
