# CLAUDE.md

This project uses **Mira** for persistent memory and code intelligence.

## Session Start

Project context is **auto-initialized** from Claude Code's working directory.
No manual `project(action="start")` call is needed.

For full session context (preferences, insights, etc.), call:
```
get_session_recap()
```

Or use `recall("preferences")` before writing code.

---

## Anti-Patterns

**NEVER** do these in the Mira codebase:

| Don't | Do Instead |
|-------|------------|
| Use `Database` directly | Use `DatabasePool` for all database access |
| Store secrets in memories | Keep secrets in `.env` only |
| Guess at MCP tool parameters | Check tool schema or existing usage first |
| Add dependencies without checking | Run `recall("dependencies")` first |
| Modify `proxy.rs` handler signatures | Coordinate changes across all tool handlers |

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
| Use external library | Guess from training data | Context7: `resolve-library-id` → `query-docs` |
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

## Task and Goal Management

### Session Workflow: Use Claude's Built-in Tasks

For current session work, use Claude Code's native task system:
- `TaskCreate` - Create tasks for multi-step work
- `TaskUpdate` - Mark in_progress/completed, set dependencies
- `TaskList` - View current session tasks

These are session-scoped and optimized for real-time workflow tracking.

### Cross-Session Planning: Use Mira Goals

For work spanning multiple sessions, use Mira's `goal` tool with milestones:

```
goal(action="create", title="Implement auth system", priority="high")
goal(action="add_milestone", goal_id="1", milestone_title="Design API", weight=2)
goal(action="add_milestone", goal_id="1", milestone_title="Implement endpoints", weight=3)
goal(action="complete_milestone", milestone_id="1")  # Auto-updates progress
goal(action="list")  # Shows goals with progress %
goal(action="get", goal_id="1")  # Shows goal details with milestones
```

**When to use goals:**
- Multi-session objectives (features, refactors, migrations)
- Tracking progress over time
- Breaking large work into weighted milestones

**Goal statuses:** planning, in_progress, blocked, completed, abandoned

**Priorities:** low, medium, high, critical

**Goal actions:**
- `create` / `bulk_create` - Create new goal(s)
- `list` / `get` - View goals and their milestones
- `update` / `progress` - Update goal fields or progress
- `delete` - Remove a goal
- `add_milestone` - Add a milestone to a goal (with optional weight)
- `complete_milestone` - Mark milestone done (auto-updates goal progress)
- `delete_milestone` - Remove a milestone

### Quick Reference

| Need | Tool |
|------|------|
| Track work in THIS session | Claude's `TaskCreate` |
| Track work across sessions | Mira's `goal` |
| Add sub-items to goal | `goal(action="add_milestone")` |
| Check long-term progress | `goal(action="list")` |

### Examples

<example>
User: I need to implement a new authentication system for the app.
Assistant: This is a significant feature that will span multiple sessions. Let me create a goal to track it.
*Uses goal(action="create", title="Implement authentication system", priority="high")*
Created goal with id 1.

Now let me add milestones for the major steps.
*Uses goal(action="add_milestone", goal_id="1", milestone_title="Design auth flow", weight=1)*
*Uses goal(action="add_milestone", goal_id="1", milestone_title="Implement JWT handling", weight=2)*
*Uses goal(action="add_milestone", goal_id="1", milestone_title="Add login/logout endpoints", weight=2)*
*Uses goal(action="add_milestone", goal_id="1", milestone_title="Write tests", weight=1)*

<reasoning>
The assistant used Mira goals because:
1. Authentication is a multi-session feature
2. Milestones break the work into trackable steps
3. Weights reflect relative effort (JWT handling > design)
4. Progress auto-calculates as milestones complete
</reasoning>
</example>

<example>
User: What's the status on the auth work we discussed yesterday?
Assistant: Let me check your goals.
*Uses goal(action="list")*
You have 1 goal in progress: "Implement authentication system" at 25% (1/4 milestones).

*Uses goal(action="get", goal_id="1")*
Shows: Design auth flow (completed), JWT handling (pending), login/logout (pending), tests (pending).

<reasoning>
The assistant used goal list/get because:
1. Goals persist across sessions
2. Milestones show granular progress
3. The 25% progress reflects the completed design milestone
</reasoning>
</example>

<example>
User: Add a comment to this function
Assistant: I'll add the comment directly.
*Uses Edit tool to add the comment*
Done.

<reasoning>
The assistant did NOT use goal tracking because:
1. This is a single, trivial task
2. It completes in one action
3. No multi-session tracking needed
</reasoning>
</example>

---

## Memory System

Use `remember` to store decisions and context. Use `recall` to retrieve them.

### Evidence Threshold

**Don't store one-off observations.** A pattern seen once is not yet a pattern. Only use `remember` for:
- Patterns observed **multiple times** across sessions
- Decisions **explicitly requested** by the user to remember
- Mistakes that caused **real problems** (not hypothetical issues)

When uncertain, don't store it. Memories accumulate and dilute recall quality.

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

Use the unified `consult_experts` tool for second opinions before major decisions:

```
consult_experts(roles=["architect"], context="...", question="...")
consult_experts(roles=["code_reviewer", "security"], context="...", question="...")  # Multiple experts
```

**Available expert roles:**
- `architect` - system design, patterns, tradeoffs
- `plan_reviewer` - validate plans before coding
- `code_reviewer` - find bugs, quality issues
- `security` - vulnerabilities, hardening
- `scope_analyst` - missing requirements, edge cases

### When to Consult Experts

1. **Before major refactoring** - `consult_experts(roles=["architect"], ...)`
2. **After writing implementation plan** - `consult_experts(roles=["plan_reviewer"], ...)`
3. **Before merging significant changes** - `consult_experts(roles=["code_reviewer"], ...)`
4. **When handling user input or auth** - `consult_experts(roles=["security"], ...)`
5. **When requirements seem incomplete** - `consult_experts(roles=["scope_analyst"], ...)`

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
| Codebase overview | `project(action="start")` output |
| External library API | Context7: `resolve-library-id` → `query-docs` |
| Literal string search | `Grep` (OK for this) |
| Exact filename pattern | `Glob` (OK for this) |

---

## Consolidated Tools Reference

Mira uses action-based tools to reduce cognitive load. Here are the consolidated tools:

### `project` - Project/Session Management
```
project(action="start", project_path="...", name="...")  # Initialize session
project(action="set", project_path="...", name="...")    # Change active project
project(action="get")                                     # Show current project
```

### `finding` - Code Review Findings
```
finding(action="list", status="pending")                  # List findings
finding(action="get", finding_id=123)                     # Get finding details
finding(action="review", finding_id=123, status="accepted", feedback="...")  # Review single
finding(action="review", finding_ids=[1,2,3], status="rejected")  # Bulk review
finding(action="stats")                                   # Get statistics
finding(action="patterns")                                # Get learned patterns
finding(action="extract")                                 # Extract patterns from accepted findings
```

### `documentation` - Documentation Tasks

Claude Code writes documentation directly (no expert system).

```
documentation(action="list", status="pending")            # List doc tasks
documentation(action="get", task_id=123)                  # Get task details + guidelines
documentation(action="complete", task_id=123)             # Mark done after writing
documentation(action="skip", task_id=123, reason="...")   # Skip a task
documentation(action="inventory")                         # Show doc inventory
documentation(action="scan")                              # Trigger doc scan
```

**Workflow:**
1. `documentation(action="list")` - See what needs docs
2. `documentation(action="get", task_id=N)` - Get source path, target path, guidelines
3. Read the source file, write the documentation
4. `documentation(action="complete", task_id=N)` - Mark done

### `consult_experts` - Expert Consultation
```
consult_experts(roles=["architect"], context="...", question="...")
consult_experts(roles=["code_reviewer", "security"], context="...")  # Multiple
```

---

## rust-analyzer Plugin

The `rust-analyzer-lsp@claude-plugins-official` plugin provides **passive background intelligence**.

**What it does:**
- Automatic diagnostics after file edits (type errors, unused variables, etc.)
- Fix suggestions inline with errors
- Surfaced via `<new-diagnostics>` in system reminders

**Usage:** Just edit `.rs` files normally. Diagnostics appear automatically if there are errors.

---

## External Library Documentation

### CRITICAL: Use Context7 for Library Questions

Before guessing at library APIs or using potentially outdated knowledge, check Context7.

**Proactive triggers - use Context7 when:**
1. **Implementing with external libraries** - Check current API before writing code
2. **Debugging library errors** - Verify correct usage patterns
3. **User asks "how do I use X"** - Get up-to-date examples
4. **Uncertain about library API** - Don't guess, look it up
5. **Library version matters** - Context7 has version-specific docs

**Workflow:**
```
resolve-library-id(libraryName="tokio", query="async runtime spawn tasks")
query-docs(libraryId="/tokio-rs/tokio", query="how to spawn async tasks")
```

### When NOT to Use Context7

- Standard library features (Rust std, Python builtins, etc.)
- You're confident in the API from recent experience
- Simple operations with well-known patterns

### Examples

<example>
User: Help me add async file reading with tokio
Assistant: Let me check the current tokio API for file operations.
*Uses resolve-library-id(libraryName="tokio", query="async file reading")*
*Uses query-docs(libraryId="/tokio-rs/tokio", query="async file reading tokio::fs")*
Found: Use `tokio::fs::read_to_string` for async file reading...

<reasoning>
The assistant used Context7 because:
1. Tokio APIs evolve and may have changed
2. Getting current examples ensures correct usage
3. Better than guessing at async patterns
</reasoning>
</example>

<example>
User: I'm getting an error with serde deserialization
Assistant: Let me check the current serde docs for the correct pattern.
*Uses resolve-library-id(libraryName="serde", query="deserialization derive")*
*Uses query-docs(libraryId="/serde-rs/serde", query="deserialize struct derive macro")*

<reasoning>
The assistant used Context7 because:
1. Serde has specific derive patterns that are easy to get wrong
2. Current docs show the correct attribute syntax
3. Helps diagnose if the error is from incorrect usage
</reasoning>
</example>

<example>
User: Use reqwest to make an HTTP POST
Assistant: I'll implement the POST request using reqwest.
*Uses resolve-library-id(libraryName="reqwest", query="HTTP POST request")*
*Uses query-docs(libraryId="/seanmonstar/reqwest", query="POST request with JSON body")*

<reasoning>
The assistant used Context7 because:
1. reqwest API has async/blocking variants
2. JSON body handling has specific patterns
3. Current docs ensure correct Client usage
</reasoning>
</example>


---

## Build & Test

```bash
cargo build --release
cargo test
```

The binary is at `target/release/mira`. Claude Code spawns it via MCP (configured in `.mcp.json`).

## Debugging

```bash
mira debug-session   # Debug project(action="start") output
mira debug-carto     # Debug cartographer module detection
```

## Environment

API keys are in `~/.mira/.env`:
- `DEEPSEEK_API_KEY` - Expert consultation (Reasoner)
- `GEMINI_API_KEY` - Embeddings (Google gemini-embedding-001)

## Claude Code Config Locations

| File | Purpose | Scope |
|------|---------|-------|
| `~/.claude.json` | Claude Code state | Global |
| `~/.claude/settings.json` | User settings (hooks, plugins) | Global |
| `~/.claude/mcp.json` | Global MCP servers | Global |
| `<project>/.mcp.json` | Project MCP servers (preferred) | Project |
| `<project>/CLAUDE.md` | Project instructions | Project |
