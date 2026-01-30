# Tool Selection: Mira vs Grep/Glob

## Use Mira Tools When

1. **Searching for code by intent** - `search_code("authentication")` finds auth code even if the word "authentication" isn't used
2. **Understanding file structure** - `get_symbols(file_path="file.rs")` lists all definitions
3. **Tracing call relationships** - `find_callers("fn_name")` / `find_callees("fn_name")` for actual call graph
4. **Recalling past decisions** - `recall("topic")` before making architectural changes
5. **Storing decisions** - `remember(content="...", category="decision")` after important choices

## Use Grep/Glob When

1. Searching for **literal strings** (error messages, UUIDs, specific constants)
2. Finding files by **exact filename pattern** when you know the name
3. Simple one-off searches that don't need semantic understanding

## Wrong vs Right

| Task | Wrong | Right |
|------|-------|-------|
| Find authentication code | `grep -r "auth"` | `search_code("authentication")` |
| What calls this function? | `grep -r "function_name"` | `find_callers("function_name")` |
| List functions in file | `grep "fn " file.rs` | `get_symbols(file_path="file.rs")` |
| Use external library | Guess from training data | Context7: `resolve-library-id` -> `query-docs` |
| Find config files | - | `glob("**/*.toml")` - OK, exact pattern |
| Find error message | - | `grep "error 404"` - OK, literal string |

Example: "Where is authentication handled?" -> use `search_code("authentication handling")`, not `grep -r "auth"`. Semantic search finds related code using different terminology.

Example: "Find where 'connection refused' is logged" -> use `Grep` with pattern `"connection refused"`. Literal string searches are Grep's strength.
