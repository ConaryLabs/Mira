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
- `get_or_generate_map_pool()` - Get cached or generate new codebase map
- `format_compact()` - Format map as compact text for LLM consumption

## Usage

The codebase map is generated during `project(action="start")` and displayed as part of the session initialization output. It shows the hierarchical module structure with purposes and key exports.
