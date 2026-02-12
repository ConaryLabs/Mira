<!-- docs/modules/mira-server/entities.md -->
# entities

Heuristic entity extraction for memory recall boosting.

## Overview

Extracts code identifiers, file paths, and crate names from text content using precompiled regexes. When a user recalls memories, entities found in both the query and stored facts are used to boost ranking scores, improving recall relevance without requiring embeddings.

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

All regexes are compiled once via `LazyLock` statics. The module is designed to run in under 1ms on typical memory content. Entity data is stored in the `entity_mentions` table (see `db/entities.rs`) and used by `recall_semantic_with_entity_boost_sync()` for ranked memory retrieval.
