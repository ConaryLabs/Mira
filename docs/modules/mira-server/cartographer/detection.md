<!-- docs/modules/mira-server/cartographer/detection.md -->
# cartographer/detection

Polyglot module detection. Dispatches to language-specific implementations to detect module boundaries, entry points, and structure.

## Key Functions

- `detect_modules()` - Detect all modules in a codebase (dispatches by language)
- `find_entry_points()` - Find entry points (`main.rs`, `lib.rs`, `index.ts`, etc.)
- `count_lines_in_module()` - Count lines of code in a module
- `resolve_import_to_module()` - Map an import path to a detected module

## Language Support

| Module | Language |
|--------|---------|
| `rust` | Rust (follows `mod` declarations, workspace detection) |
| `python` | Python (packages, `__init__.py`) |
| `node` | TypeScript/JavaScript (package.json, index files) |
| `go` | Go (packages, go.mod) |
| `java` | Java — detected only (no code intelligence; pom.xml/build.gradle) |
