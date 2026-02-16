<!-- docs/modules/mira-server/indexer/parsers.md -->
# indexer/parsers

Tree-sitter based code parsing framework. Provides a unified interface for extracting symbols, imports, and call relationships from source files.

## Key Types

- `LanguageParser` - Trait defining the parser interface
- `ParserRegistry` - Registry of available parsers (with `by_extension()`, `by_language()`, `all()` methods)
- `Symbol` / `Import` / `FunctionCall` - Parsed code elements
- `ParsedFile` - Parsed file result
- `ParseResult` - Type alias for parsing results
- `ParseContext` - Unified parsing context (replaces multiple parameters)
- `SymbolBuilder` - Fluent API for constructing symbols
- `NodeExt` - Extension trait for tree-sitter node helpers

## Language Parsers

| Module | Language | Extensions |
|--------|----------|-----------|
| `rust` | Rust | `.rs` |
| `python` | Python | `.py` |
| `typescript` | TypeScript/JavaScript | `.ts`, `.tsx`, `.js`, `.jsx` |
| `go` | Go | `.go` |
