# mira-types

Shared data contracts between the Mira server and clients. WASM-compatible types with no native-only dependencies.

## Public Types

| Type | Kind | Description |
|------|------|-------------|
| `ProjectContext` | struct | Project identity: database ID, filesystem path, display name |
| `MemoryFact` | struct | Semantic memory unit with evidence lifecycle, scoping, and confidence |
| `AgentRole` | enum | Agent identity: `Mira` (local orchestrator) or `Claude` (remote LLM) |
| `WsEvent` | enum | WebSocket protocol events: `ToolStart`, `ToolResult`, `AgentResponse` |

## Design

All types derive `Serialize`/`Deserialize` for JSON transport. The crate has no dependencies on native-only code, making it suitable for WASM targets.

See individual type documentation in [`docs/api/`](../api/) for detailed field descriptions and serialization examples.
