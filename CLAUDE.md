# CLAUDE.md

This project uses **Mira** for persistent memory and code intelligence.

## Session Start

```
session_start(project_path="/home/peter/Mira")
```

Then `recall("preferences")` before writing code.

## Quick Reference

| Need | Tool |
|------|------|
| Find definition | `cclsp_find_definition` |
| Find usages | `cclsp_find_references` |
| Search by meaning | `semantic_code_search` |
| Check past decisions | `recall` |
| File structure | `get_symbols` |

## Build

```bash
cargo build --release
```
