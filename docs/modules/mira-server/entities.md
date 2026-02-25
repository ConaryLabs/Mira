<!-- docs/modules/mira-server/entities.md -->
# entities

Heuristic entity extraction from text content.

## Overview

Extracts code identifiers, file paths, and crate names from text content using precompiled regexes. Used by subagent hooks to extract entities from subagent output for discovery tracking.

## Key Types

- `RawEntity` -- An extracted entity with original name, canonical form, and type
- `EntityType` -- Classification enum: `CodeIdent`, `FilePath`, `CrateName`

## Key Functions

- `extract_entities_heuristic(content)` -- Extract all entities from text, deduplicated by (canonical_name, entity_type)
- `normalize_entity(name)` -- Normalize an identifier to canonical form (CamelCase to snake_case, hyphens to underscores, collapse duplicates)

## Extraction Patterns

1. **File paths** -- Matches paths ending in known extensions (.rs, .ts, .py, .go, etc.)
2. **Backtick code refs** -- Content inside backticks, with trailing `()` or `:line_number` stripped
3. **CamelCase identifiers** -- 2+ humps, minimum 5 characters
4. **snake_case identifiers** -- 2+ segments, minimum 5 characters
5. **Crate names** -- After `crate`/`use`/`mod` keywords

## Architecture Notes

All regexes are compiled once via `LazyLock` statics. The module is designed to run in under 1ms on typical content.
