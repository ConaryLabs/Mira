<!-- .claude/rules/tool-selection.md -->

# Tool Selection: Mira vs Grep/Glob

## Use Mira's `run()` Tool When

1. **Searching for code by intent** - `run('search("authentication")')` finds auth code even if the word "authentication" isn't used
2. **Understanding file structure** - `run('symbols("file.rs")')` lists all definitions
3. **Tracing call relationships** - `run('callers("fn_name")')` / `run('callees("fn_name")')` for actual call graph

## Use Grep/Glob When

1. Searching for **literal strings** (error messages, UUIDs, specific constants)
2. Finding files by **exact filename pattern** when you know the name
3. Simple one-off searches that don't need semantic understanding

## Wrong vs Right

| Task | Wrong | Right |
|------|-------|-------|
| Find authentication code | `grep -r "auth"` | `run('search("authentication")')` |
| What calls this function? | `grep -r "function_name"` | `run('callers("function_name")')` |
| List functions in file | `grep "fn " file.rs` | `run('symbols("file.rs")')` |
| Use external library | Guess from training data | Context7: `resolve-library-id` -> `query-docs` |
| Find config files | - | `glob("**/*.toml")` - OK, exact pattern |
| Find error message | - | `grep "error 404"` - OK, literal string |

Example: "Where is authentication handled?" -> use `run('search("authentication handling")')`, not `grep -r "auth"`. Semantic search finds related code using different terminology.

Example: "Find where 'connection refused' is logged" -> use `Grep` with pattern `"connection refused"`. Literal string searches are Grep's strength.

Use `run('help()')` to list all available Rhai functions.
