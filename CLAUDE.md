# CLAUDE.md

This project uses **Mira** for persistent memory and code intelligence.

## Session Start

```
session_start(project_path="/home/peter/Mira")
```

Then `recall("preferences")` before writing code.

---

## CRITICAL: Tool Selection

STOP before using Grep or Glob. Use Mira tools instead.

### When to Use Mira Tools

Use Mira tools proactively in these scenarios:

1. **Searching for code by intent** - Use `semantic_code_search` instead of Grep
2. **Understanding file structure** - Use `get_symbols` instead of grepping for definitions
3. **Tracing call relationships** - Use `find_callers` / `find_callees` instead of grepping function names
4. **Checking if a feature exists** - Use `check_capability` instead of exploratory grep
5. **Recalling past decisions** - Use `recall` before making architectural changes
6. **Storing decisions for future sessions** - Use `remember` after important choices

### When NOT to Use Mira Tools

Use Grep/Glob directly only when:

1. Searching for **literal strings** (error messages, UUIDs, specific constants)
2. Finding files by **exact filename pattern** when you know the name
3. The search is a simple one-off that doesn't need semantic understanding

### Wrong vs Right

| Task | Wrong | Right |
|------|-------|-------|
| Find authentication code | `grep -r "auth"` | `semantic_code_search("authentication")` |
| What calls this function? | `grep -r "function_name"` | `find_callers("function_name")` |
| List functions in file | `grep "fn " file.rs` | `get_symbols(file_path="file.rs")` |
| Check if feature exists | `grep -r "feature"` | `check_capability("feature description")` |
| Find config files | `find . -name "*.toml"` | `glob("**/*.toml")` - OK, exact pattern |
| Find error message | `semantic_code_search("error 404")` | `grep "error 404"` - OK, literal string |

### Examples

<example>
User: Where is authentication handled in this codebase?
Assistant: Let me search for authentication-related code using semantic search.
*Uses semantic_code_search("authentication handling login session")*
Found authentication logic in `src/auth/middleware.rs` and `src/handlers/login.rs`.

<reasoning>
The assistant used semantic_code_search because:
1. "Authentication" is a concept, not a literal string
2. The code might use various terms: auth, login, session, credentials, etc.
3. Semantic search finds related code even if the exact word "authentication" isn't used
4. Grep would miss files that implement auth without using that exact term
</reasoning>
</example>

<example>
User: What functions call the `process_request` function?
Assistant: Let me find all callers of process_request.
*Uses find_callers("process_request")*
Found 5 callers across 3 files: middleware.rs, handlers.rs, and tests.rs.

<reasoning>
The assistant used find_callers because:
1. It needs the call graph, not just text matches
2. Grep would find the definition and string mentions, not actual calls
3. find_callers uses code analysis to identify real invocations
4. This is faster and more accurate than manual grep + filtering
</reasoning>
</example>

<example>
User: Does this project have caching implemented?
Assistant: Let me check if caching capability exists.
*Uses check_capability("caching layer for database queries or API responses")*
Found: Redis caching in `src/cache/redis.rs`, in-memory LRU cache in `src/cache/memory.rs`.

<reasoning>
The assistant used check_capability because:
1. The user wants to know IF something exists, not find a specific string
2. Caching could be implemented in many ways with different terms
3. check_capability searches semantically and returns a summary
4. Grep -r "cache" would return too many false positives
</reasoning>
</example>

<example>
User: Find where the error "connection refused" is logged
Assistant: Let me grep for that exact error message.
*Uses Grep with pattern "connection refused"*
Found in `src/net/client.rs:145` and `src/net/server.rs:89`.

<reasoning>
The assistant used Grep because:
1. This is a literal string search, not a semantic concept
2. The exact error message text is known
3. Grep is the right tool for finding exact string matches
4. Semantic search would be overkill for literal text
</reasoning>
</example>

---

## Task Management

Use Mira's `task` and `goal` tools for **cross-session persistence**. This is the PRIMARY task management system for this project.

### When to Use Mira Tasks

Use Mira tasks proactively in these scenarios:

1. **Multi-session work** - Any task that might span multiple conversations
2. **Complex multi-step tasks** - When work requires 3+ distinct steps
3. **User provides multiple tasks** - Capture all items for tracking
4. **Before starting implementation** - Create tasks to track progress
5. **Architectural decisions** - Track decisions that affect future work
6. **Bugs and issues** - Log issues discovered during development

### When NOT to Use Mira Tasks

Use TodoWrite (or nothing) when:

1. Single, trivial task completable in one response
2. Pure information/explanation requests
3. Quick one-off commands (run build, check status)

### Task Tool Usage

```
task(action="create", title="Implement user authentication", priority="high")
task(action="list")  # See pending tasks from previous sessions
task(action="update", task_id="123", status="completed")
goal(action="create", title="v2.0 Release", description="Major feature milestone")
```

### Examples

<example>
User: I need to refactor the database layer to use connection pooling, add retry logic, and update all the queries.
Assistant: This is a multi-step task that may span sessions. Let me create tasks to track this work.
*Uses task(action="bulk_create", tasks=[...]) to create:*
1. Research connection pooling options for SQLite
2. Implement connection pool wrapper
3. Add retry logic with exponential backoff
4. Update query functions to use pool
5. Add integration tests for pool behavior
6. Update documentation

Let me also check if there are any existing tasks related to database work.
*Uses task(action="list")*

<reasoning>
The assistant used Mira tasks because:
1. This refactoring will likely span multiple sessions
2. There are 6 distinct steps that need tracking
3. Mira tasks persist across sessions, unlike TodoWrite
4. If the conversation ends, the task list survives for the next session
5. Checking existing tasks avoids duplicate work
</reasoning>
</example>

<example>
User: What's the status on the work we discussed yesterday?
Assistant: Let me check your pending tasks and goals.
*Uses task(action="list") and goal(action="list")*
You have 3 pending tasks from yesterday's session and 1 goal in progress.

<reasoning>
The assistant used Mira task list because:
1. The user is asking about cross-session work
2. Mira tasks persist between sessions
3. TodoWrite would be empty since it's a new session
4. This demonstrates the value of persistent task tracking
</reasoning>
</example>

<example>
User: Add a comment to this function
Assistant: I'll add the comment directly.
*Uses Edit tool to add the comment*
Done. Added the comment explaining the function's purpose.

<reasoning>
The assistant did NOT use task tracking because:
1. This is a single, trivial task
2. It completes in one action
3. No multi-step tracking needed
4. Creating a task would be overhead with no benefit
</reasoning>
</example>

---

## Memory System

Use `remember` to store decisions and context. Use `recall` to retrieve them.

### When to Use Memory

1. **After architectural decisions** - Store the decision and reasoning
2. **User preferences discovered** - Store for future sessions
3. **Mistakes made and corrected** - Remember to avoid repeating
4. **Before making changes** - Recall past decisions in that area
5. **Workflows that worked** - Store successful patterns

### Examples

<example>
User: Let's use the builder pattern for the Config struct instead of the current approach.
Assistant: I'll implement the builder pattern for Config.
*Implements the changes*
Let me store this decision for future reference.
*Uses remember(content="Config struct uses builder pattern. Chosen over direct construction for clarity and optional field handling.", category="decision")*

<reasoning>
The assistant used remember because:
1. This is an architectural decision that affects future code
2. Future sessions might wonder why builder pattern was chosen
3. Storing the reasoning prevents revisiting the same decision
4. Category "decision" makes it easy to recall later
</reasoning>
</example>

<example>
User: Help me add a new endpoint to the API
Assistant: Before I start, let me check for any past decisions about API design.
*Uses recall("API design patterns endpoints conventions")*
Found: "API uses REST conventions with /api/v1 prefix. All endpoints return JSON with consistent error format."

I'll follow these established patterns for the new endpoint.

<reasoning>
The assistant used recall because:
1. API design decisions were likely made in past sessions
2. Following existing patterns ensures consistency
3. The recall prevents accidentally introducing different conventions
4. This is the "before making changes" use case
</reasoning>
</example>

---

## Sub-Agent Context Injection

When spawning sub-agents (Task tool with Explore, Plan, etc.), they do NOT automatically have access to Mira memories. You must inject relevant context into the prompt.

### Pattern: Recall Before Task

Before launching a sub-agent for significant work:

1. Use `recall()` to get relevant context
2. Include key information in the Task prompt
3. Be explicit about project conventions

### Examples

<example>
User: Explore how error handling works in this codebase
Assistant: Let me first recall any past decisions about error handling, then launch an explore agent with that context.
*Uses recall("error handling patterns conventions")*
Found: "Project uses thiserror for error types. All errors implement std::error::Error. Custom MiraError enum in types crate."

*Uses Task tool with Explore agent:*
Prompt: "Find how error handling works in this codebase. Context: The project uses thiserror for error types, has a custom MiraError enum in the types crate, and all errors implement std::error::Error. Look for error propagation patterns and any Result type aliases."

<reasoning>
The assistant injected context because:
1. The Explore agent doesn't see Mira memories
2. Past decisions about error handling are relevant
3. Including context helps the agent search more effectively
4. The agent can now look for specific patterns (thiserror, MiraError)
</reasoning>
</example>

<example>
User: Plan out implementing a new caching layer
Assistant: Let me gather context before planning.
*Uses recall("caching architecture performance")*
*Uses recall("database layer design")*
*Uses check_capability("existing caching")*

Now I'll launch a Plan agent with this context.
*Uses Task tool with Plan agent:*
Prompt: "Design an implementation plan for adding a caching layer. Context from past sessions:
- Database uses SQLite with custom connection wrapper
- No existing caching layer found
- Performance optimization is a priority
- User preference: avoid adding heavy dependencies

Consider: cache invalidation strategy, where to add caching (query level vs application level), storage backend options."

<reasoning>
The assistant gathered context first because:
1. Plan agent needs project context to make good recommendations
2. Past decisions constrain the design space
3. User preferences affect the approach (no heavy deps)
4. The agent can now plan with full awareness of constraints
</reasoning>
</example>

---

## Expert Consultation

Use experts for second opinions before major decisions:

- `consult_architect` - system design, patterns, tradeoffs
- `consult_plan_reviewer` - validate plans before coding
- `consult_code_reviewer` - find bugs, quality issues
- `consult_security` - vulnerabilities, hardening
- `consult_scope_analyst` - missing requirements, edge cases

### When to Consult Experts

1. **Before major refactoring** - Get architectural review
2. **After writing implementation plan** - Validate with plan_reviewer
3. **Before merging significant changes** - Code review
4. **When handling user input or auth** - Security review
5. **When requirements seem incomplete** - Scope analysis

---

## Code Navigation Quick Reference

| Need | Tool |
|------|------|
| Search by meaning | `semantic_code_search` |
| File structure | `get_symbols` |
| What calls X? | `find_callers` |
| What does X call? | `find_callees` |
| Past decisions | `recall` |
| Feature exists? | `check_capability` |
| Codebase overview | `session_start` output |
| Library documentation | `resolve-library-id` + `query-docs` |
| Literal string search | `Grep` (OK for this) |
| Exact filename pattern | `Glob` (OK for this) |

---

## rust-analyzer LSP Plugin

The `rust-analyzer@claude-code-lsps` plugin is enabled in `~/.claude/settings.json`. It provides **passive background intelligence** - not directly callable tools.

**What it does:**
- Automatic diagnostics after file edits (type errors, unused variables, etc.)
- Fix suggestions inline with errors
- Surfaced via `<new-diagnostics>` in system reminders

**Mira vs LSP:**

| Capability | Mira | LSP |
|------------|------|-----|
| Invocation | Explicit tool calls | Automatic after edits |
| Diagnostics | No | Yes, with fix suggestions |
| Semantic search | Yes | No |
| Memory/context | Yes | No |

**Usage:** Just edit `.rs` files normally. Diagnostics appear automatically if there are errors. No explicit invocation needed.

---

## Documentation MCP Servers

### Context7
- Provides up-to-date documentation and code examples for any library
- Tools: `resolve-library-id` and `query-docs`
- Always call `resolve-library-id` first to get library ID unless user provides ID in format `/org/project`
- Auto-invoke when user asks about library documentation

### OpenAI Docs
- Provides documentation for OpenAI API, SDKs, and related tools
- Tools: `search_openai_docs`, `fetch_openai_doc`, `list_openai_docs`, `list_api_endpoints`, `get_openapi_spec`
- Use when working with OpenAI API, SDKs, ChatGPT Apps SDK, or Codex

---

## Build & Test

```bash
cargo build --release
cargo test
```

The binary is at `target/release/mira`. Claude Code spawns it via MCP (configured in `.mcp.json`).

## Debugging

```bash
mira debug-session   # Debug session_start output
mira debug-carto     # Debug cartographer module detection
```

## Environment

API keys are in `/home/peter/Mira/.env`:
- `OPENAI_API_KEY` - Embeddings (text-embedding-3-small)
- `DEEPSEEK_API_KEY` - Expert consultation (Reasoner)

## Claude Code Config Locations

| File | Purpose | Scope |
|------|---------|-------|
| `~/.claude.json` | Claude Code state | Global |
| `~/.claude/settings.json` | User settings (hooks, plugins) | Global |
| `~/.claude/mcp.json` | Global MCP servers | Global |
| `<project>/.mcp.json` | Project MCP servers (preferred) | Project |
| `<project>/CLAUDE.md` | Project instructions | Project |
