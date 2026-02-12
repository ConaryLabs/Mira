<!-- docs/modules/mira-server/tools/core/claude_local.md -->
# tools/core/claude_local

Bidirectional sync between Mira memories and CLAUDE.local.md files.

## Overview

Manages the export of Mira memories to CLAUDE.local.md (so Claude Code can read them as system context) and the import of manually edited CLAUDE.local.md content back into memory. Memories are classified into sections (Preferences, Decisions, Patterns, General) and rendered as markdown with budget-aware truncation.

## Key Functions

- `export_claude_local()` - Export memories to CLAUDE.local.md with budget packing
- `build_budgeted_export()` - Build export content within a byte budget
- `write_claude_local_md_sync()` - Write the export to disk
- `import_claude_local_md_async()` - Import manually added content back into memory
- `parse_claude_local_md()` - Parse CLAUDE.local.md into sections and entries
- `write_auto_memory_sync()` - Write to Claude Code's auto memory directory

## Sub-modules

| Module | Purpose |
|--------|---------|
| `export` | Memory export and budget packing |
| `import` | CLAUDE.local.md parsing and memory import |
| `auto_memory` | Integration with Claude Code's `~/.claude/projects/` auto memory |

## Architecture Notes

Memories are ranked by confidence and session count, then packed into sections within a configurable byte budget. Content is truncated at character boundaries to avoid breaking multi-byte characters. The classifier maps `fact_type` and `category` fields to display sections. Called via `memory(action="export_claude_local")` and automatically by the `Stop` hook at session end.
