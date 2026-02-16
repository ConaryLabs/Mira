<!-- docs/modules/mira-server/indexer.md -->
# indexer

Code indexing for symbol extraction, call graph construction, and semantic search chunking.

## Supported Languages

| Language | Extensions |
|----------|-----------|
| Rust | `.rs` |
| Python | `.py` |
| TypeScript/JavaScript | `.ts`, `.tsx`, `.js`, `.jsx` |
| Go | `.go` |

## Sub-modules

| Module | Purpose |
|--------|---------|
| `parsers` | Language-specific parsers |
| `parsing` | Parse orchestration and language detection |
| `chunking` | Code chunking for embedding generation |
| `project` | Project-level indexing |
| `batch` | Batch processing |
| `types` | Core types (`CodeChunk`, `ParsedSymbol`, etc.) |

## Key Types

- `CodeChunk` - A chunk of code for embedding
- `ParsedSymbol` - A parsed symbol definition
- `FileParseResult` - Result of parsing a single file
- `IndexStats` - Statistics from an indexing run
- `ParsedImport` - A parsed import statement

## Key Functions

- `index_project()` - Index an entire project directory
- `parse_file()` - Parse a single file for symbols
- `extract_symbols()` - Extract symbols (functions, structs, classes) from a file
- `extract_all()` - Extract symbols, imports, and call relationships

**Note:** Some modules are behind the `#[cfg(feature = "parsers")]` feature gate.

## Output

Indexing produces:
- **Symbols** - Functions, structs, classes, traits with line ranges
- **Call graph** - Which functions call which other functions
- **Import graph** - Module/package import relationships
- **Chunks** - Code segments for embedding and semantic search
