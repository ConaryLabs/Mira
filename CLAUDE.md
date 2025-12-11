# CLAUDE.md

This project uses **Mira** - a persistent memory and intelligence layer via MCP.

## Getting Mira Guidance

At the start of each session, call:
```
get_guidelines(category="mira_usage")
```

This returns instructions on when and how to use Mira's tools (remember, recall, get_related_files, etc.).

## Quick Reference

**Essential tools:**
- `remember(content, fact_type, category)` - Store preferences, decisions, context
- `recall(query)` - Search memories semantically
- `get_guidelines(category)` - Get stored guidelines (use category="mira_usage" for Mira instructions)

## Development

```bash
# Build
SQLX_OFFLINE=true cargo build --release

# Run
DATABASE_URL="sqlite://data/mira.db" ./target/release/mira
```

## Structure

```
src/
├── main.rs       # MCP server (36 tools)
└── tools/        # Tool implementations
migrations/       # Database schema
```
