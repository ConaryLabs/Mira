# AgentRole

Identifies which agent produced a message or action in the Mira collaboration system.

**Crate:** `mira-types`
**Source:** `crates/mira-types/src/lib.rs`

## Definition

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    Mira,
    Claude,
}
```

## Variants

| Variant | JSON Value | Description |
|---------|-----------|-------------|
| `Mira` | `"mira"` | The local Mira orchestrator/server |
| `Claude` | `"claude"` | The remote LLM intelligence (Claude) |

## Traits

`Debug`, `Clone`, `Copy`, `Serialize`, `Deserialize`, `PartialEq`, `Eq`

## Usage

`AgentRole` is used in `WsEvent::AgentResponse` to identify the source of a collaboration message:

```rust
WsEvent::AgentResponse {
    in_reply_to: "msg-123".into(),
    from: AgentRole::Claude,
    content: "Analysis complete.".into(),
    complete: true,
}
```

## Serialization

Uses snake_case via `#[serde(rename_all = "snake_case")]`:

```json
{ "from": "claude" }
{ "from": "mira" }
```

## See Also

- [WsEvent](WsEvent.md) - Uses `AgentRole` in the `AgentResponse` variant
