# get_symbols

Get symbols (functions, structs, classes, etc.) from a file. Parses the file and returns a list of detected symbols with their types and line locations.

## Usage

```json
{
  "name": "get_symbols",
  "arguments": {
    "file_path": "src/main.rs"
  }
}
```

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| file_path | String | Yes | Path to the file to analyze |
| symbol_type | String | No | Filter by symbol type (e.g. `function`, `struct`, `class`, `impl`) - case-insensitive |

## Returns

Formatted list of symbols with types and line ranges. Limited to the first 10 symbols in display, with a count of remaining.

```
15 symbols:
  get_symbols (function) lines 193-236
  search_code (function) lines 18-121
  find_function_callers (function) lines 124-156
  MiraServer (struct) line 42
  ... and 11 more
```

Or: `No symbols found.`

## Examples

**Example 1: List all symbols in a file**
```json
{
  "name": "get_symbols",
  "arguments": { "file_path": "crates/mira-server/src/mcp/mod.rs" }
}
```

**Example 2: Filter to only functions**
```json
{
  "name": "get_symbols",
  "arguments": {
    "file_path": "src/lib.rs",
    "symbol_type": "function"
  }
}
```

## Errors

- **"File not found: {path}"**: The specified file does not exist.
- **Parse errors**: The file could not be parsed for symbols.

## See Also

- **search_code**: Search code by semantic meaning
- **find_callers** / **find_callees**: Trace call relationships between functions
- **index**: Index the full project for comprehensive symbol data
