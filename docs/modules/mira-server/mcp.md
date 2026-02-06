# mcp

MCP (Model Context Protocol) server implementation. Exposes all of Mira's intelligence tools to Claude Code via the standardized MCP protocol.

## Implementation

Built on the **`rmcp`** library (Rust MCP). Implements the `ServerHandler` trait with a `ToolRouter` for dispatching tool calls.

## Files

| File | Purpose |
|------|---------|
| `mod.rs` | `MiraServer` struct and server state |
| `router.rs` | Tool registrations with `#[tool]` attribute macros |
| `handler.rs` | MCP protocol handler implementation |
| `requests.rs` | Request type definitions (action enums, parameter structs) |
| `responses/` | Response type definitions (structured JSON output schemas) |
| `extraction.rs` | Tool outcome extraction and memory capture |
| `client.rs` | MCP client for accessing host environment tools from experts |
| `elicitation.rs` | Interactive user input (API key setup) |
| `tasks.rs` | Async long-running task management |

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
