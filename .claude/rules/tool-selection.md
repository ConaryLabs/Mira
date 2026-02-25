# Tool Selection: Mira vs Grep/Glob

## Use Mira Tools When

1. **Searching for code by intent** - `code(action="search", query="authentication")` finds auth code even if the word "authentication" isn't used
2. **Understanding file structure** - `code(action="symbols", file_path="file.rs")` lists all definitions
3. **Tracing call relationships** - `code(action="callers", function_name="fn_name")` / `code(action="callees", function_name="fn_name")` for actual call graph

## Use Grep/Glob When

1. Searching for **literal strings** (error messages, UUIDs, specific constants)
2. Finding files by **exact filename pattern** when you know the name
3. Simple one-off searches that don't need semantic understanding

## Wrong vs Right

| Task | Wrong | Right |
|------|-------|-------|
| Find authentication code | `grep -r "auth"` | `code(action="search", query="authentication")` |
| What calls this function? | `grep -r "function_name"` | `code(action="callers", function_name="function_name")` |
| List functions in file | `grep "fn " file.rs` | `code(action="symbols", file_path="file.rs")` |
| Use external library | Guess from training data | Context7: `resolve-library-id` -> `query-docs` |
| Find config files | - | `glob("**/*.toml")` - OK, exact pattern |
| Find error message | - | `grep "error 404"` - OK, literal string |

Example: "Where is authentication handled?" -> use `code(action="search", query="authentication handling")`, not `grep -r "auth"`. Semantic search finds related code using different terminology.

Example: "Find where 'connection refused' is logged" -> use `Grep` with pattern `"connection refused"`. Literal string searches are Grep's strength.
