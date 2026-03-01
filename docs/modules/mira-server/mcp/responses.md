<!-- docs/modules/mira-server/mcp/responses.md -->
# mcp/responses

Structured output types for MCP tools.

## Overview

Defines typed response schemas for every MCP tool. Each tool returns a `ToolOutput<D>` wrapper containing an `action` string, human-readable `message`, and optional typed `data`. The `Json<T>` wrapper implements `IntoCallToolResult` to produce both a text content summary and structured JSON output, enabling rmcp to auto-infer `outputSchema` for each tool.

## Key Types

- `ToolOutput<D>` -- Generic tool output with action, message, and optional data
- `Json<T>` -- Wrapper that converts typed output into MCP `CallToolResult` with both text and structured content
- `HasMessage` -- Trait for outputs that expose a human-readable message

## Sub-modules

Each file defines the data types for one tool's responses:

| Module | Output Type | Tool |
|--------|-------------|------|
| `memory` | `MemoryOutput` | memory |
| `project` | `ProjectOutput` | project |
| `code` | `CodeOutput` | code |
| `goal` | `GoalOutput` | goal |
| `index` | `IndexOutput` | index |
| `session` | `SessionOutput` | session |
| `documentation` | `DocOutput` | documentation |
| `diff` | `DiffOutput` | diff |
| `team` | `TeamOutput` | team |
| `tasks` | `TasksOutput` | tasks |

## Architecture Notes

All output types must produce a root "object" JSON schema (MCP requirement). This is validated by a test that calls `schema_for_output::<T>()` for every output type. The module re-exports all domain types from the root so existing `use crate::mcp::responses::X` imports continue to work.
