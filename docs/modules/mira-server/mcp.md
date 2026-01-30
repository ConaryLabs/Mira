# mcp

MCP (Model Context Protocol) server implementation. Exposes all of Mira's intelligence tools to Claude Code via the standardized MCP protocol.

## Implementation

Built on the **`rmcp`** library (Rust MCP). Implements the `ServerHandler` trait with a `ToolRouter` for dispatching tool calls.

## Files

| File | Purpose |
|------|---------|
| `mod.rs` | `MiraServer` struct and tool registrations |
| `requests.rs` | Request type definitions (action enums, parameter structs) |
| `extraction.rs` | Request parameter extraction logic |

## Key Type: MiraServer

The central server state holding:

- `pool` / `code_pool` - Database connection pools (main + code index)
- `embeddings` - Embedding client for semantic search
- `llm_factory` - LLM provider factory for expert consultation
- `project` - Current project context
- `session_id` / `branch` - Session tracking
- `mcp_client_manager` - Connections to external MCP servers
- `pending_responses` - Agent collaboration message queue
- `tool_router` - Routes MCP tool calls to handlers

## Tool Registration

Tools are registered as methods on `MiraServer` with the `#[tool]` attribute macro from `rmcp`. Each tool method deserializes a typed request struct and delegates to the corresponding handler in `tools::core`.
