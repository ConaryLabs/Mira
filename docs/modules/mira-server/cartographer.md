<!-- docs/modules/mira-server/cartographer.md -->
# cartographer

Codebase mapping and structure analysis. Detects modules, entry points, and dependencies to generate a navigable codebase map.

## Sub-modules

| Module | Purpose |
|--------|---------|
| `detection` | Module detection, workspace detection, entry point finding |
| `map` | Map generation and retrieval |
| `summaries` | Module summaries and purpose extraction |
| `types` | Core types (`CodebaseMap`, `Module`, etc.) |

## Key Functions

- `detect_modules()` - Detect all modules in a codebase
- `detect_rust_modules()` - Rust-specific module detection (follows `mod` declarations)
- `detect_entry_points()` - Find entry points (`main.rs`, `lib.rs`, etc.)
- `is_workspace` - Check if project is a Cargo workspace
- `parse_crate_name` - Extract crate name from Cargo.toml
- `get_or_generate_map_pool()` - Get cached or generate new codebase map
- `get_modules_with_purposes_pool()` - Get modules with their purpose descriptions
- `get_modules_needing_summaries()` - Find modules needing LLM summary generation
- `build_summary_prompt()` / `parse_summary_response()` / `update_module_purposes()` - Summary generation pipeline
- `get_module_code_preview()` / `get_module_full_code()` - Module source code access
- `format_compact()` - Format map as compact text for LLM consumption

## Key Types

- `ModuleSummaryContext` - Context for summary generation

## Usage

The codebase map is generated during `project(action="start")` and displayed as part of the session initialization output. It shows the hierarchical module structure with purposes and key exports.
