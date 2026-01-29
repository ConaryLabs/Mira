# CLAUDE.md Template for Mira

Copy the sections below into your project's `CLAUDE.md` file to get the most out of Mira.

---

## Minimal Setup

At minimum, add this to your `CLAUDE.md`:

```markdown
# CLAUDE.md

This project uses **Mira** for persistent memory and code intelligence.

## Session Start

Project context is **auto-initialized** from Claude Code's working directory.
No manual `project(action="start")` call is needed.

For full session context (preferences, insights, etc.), call:
get_session_recap()

Or use `recall("preferences")` before writing code.
```

---

## Recommended Setup

For best results, include the full tool selection guidance:

```markdown
# CLAUDE.md

This project uses **Mira** for persistent memory and code intelligence.

## Session Start

Project context is **auto-initialized** from Claude Code's working directory.
No manual `project(action="start")` call is needed.

For full session context (preferences, insights, etc.), call:
get_session_recap()

Or use `recall("preferences")` before writing code.

---

## CRITICAL: Tool Selection

STOP before using Grep or Glob. Use Mira tools instead.

### When to Use Mira Tools

Use Mira tools proactively in these scenarios:

1. **Searching for code by intent** - Use `search_code` instead of Grep
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
| Find authentication code | `grep -r "auth"` | `search_code("authentication")` |
| What calls this function? | `grep -r "function_name"` | `find_callers("function_name")` |
| List functions in file | `grep "fn " file.rs` | `get_symbols(file_path="file.rs")` |
| Check if feature exists | `grep -r "feature"` | `check_capability("feature description")` |
| Use external library | Guess from training data | Context7: `resolve-library-id` -> `query-docs` |
| Find config files | `find . -name "*.toml"` | `glob("**/*.toml")` - OK, exact pattern |
| Find error message | `search_code("error 404")` | `grep "error 404"` - OK, literal string |

---

## Task and Goal Management

### Session Workflow: Use Claude's Built-in Tasks

For current session work, use Claude Code's native task system:
- `TaskCreate` - Create tasks for multi-step work
- `TaskUpdate` - Mark in_progress/completed, set dependencies
- `TaskList` - View current session tasks

### Cross-Session Planning: Use Mira Goals

For work spanning multiple sessions, use Mira's `goal` tool with milestones:

goal(action="create", title="Implement feature X", priority="high")
goal(action="add_milestone", goal_id="1", milestone_title="Design API", weight=2)
goal(action="complete_milestone", milestone_id="1")  # Auto-updates progress
goal(action="list")  # Shows goals with progress %

**When to use goals:**
- Multi-session objectives (features, refactors, migrations)
- Tracking progress over time
- Breaking large work into weighted milestones

### Quick Reference

| Need | Tool |
|------|------|
| Track work in THIS session | Claude's `TaskCreate` |
| Track work across sessions | Mira's `goal` |
| Add sub-items to goal | `goal(action="add_milestone")` |
| Check long-term progress | `goal(action="list")` |

---

## Memory System

Use `remember` to store decisions and context. Use `recall` to retrieve them.

### Evidence Threshold

**Don't store one-off observations.** Only use `remember` for:
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

---

## Sub-Agent Context Injection

When spawning sub-agents (Task tool with Explore, Plan, etc.), they do NOT automatically have access to Mira memories. You must inject relevant context into the prompt.

### Pattern: Recall Before Task

Before launching a sub-agent for significant work:

1. Use `recall()` to get relevant context
2. Include key information in the Task prompt
3. Be explicit about project conventions

---

## Expert Consultation

Use the unified `consult_experts` tool for second opinions before major decisions:

consult_experts(roles=["architect"], context="...", question="...")
consult_experts(roles=["code_reviewer", "security"], context="...")  # Multiple experts

**Available expert roles:**
- `architect` - system design, patterns, tradeoffs
- `plan_reviewer` - validate plans before coding
- `code_reviewer` - find bugs, quality issues
- `security` - vulnerabilities, hardening
- `scope_analyst` - missing requirements, edge cases
- `documentation_writer` - generate comprehensive documentation

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
| Search by meaning | `search_code` |
| File structure | `get_symbols` |
| What calls X? | `find_callers` |
| What does X call? | `find_callees` |
| Past decisions | `recall` |
| Feature exists? | `check_capability` |
| Codebase overview | `project(action="start")` output |
| External library API | Context7: `resolve-library-id` -> `query-docs` |
| Literal string search | `Grep` (OK for this) |
| Exact filename pattern | `Glob` (OK for this) |

---

## Consolidated Tools Reference

Mira uses action-based tools. Here are the key ones:

### `project` - Project/Session Management
project(action="start", project_path="...", name="...")  # Initialize session
project(action="set", project_path="...", name="...")    # Change active project
project(action="get")                                     # Show current project

### `goal` - Cross-Session Goals
goal(action="create", title="...", priority="high")       # Create goal
goal(action="list")                                       # List goals
goal(action="add_milestone", goal_id="1", milestone_title="...", weight=2)
goal(action="complete_milestone", milestone_id="1")       # Mark done

### `finding` - Code Review Findings
finding(action="list", status="pending")                  # List findings
finding(action="review", finding_id=123, status="accepted", feedback="...")
finding(action="stats")                                   # Get statistics

### `documentation` - Documentation Tasks
documentation(action="list", status="pending")            # List doc tasks
documentation(action="skip", task_id=123, reason="...")   # Skip a task
documentation(action="inventory")                         # Show doc inventory

### `consult_experts` - Expert Consultation
consult_experts(roles=["architect"], context="...", question="...")
consult_experts(roles=["code_reviewer", "security"], context="...")
```

---

## Adding Project-Specific Content

After the Mira sections above, add your own project-specific content:

- Build & Test commands
- Architecture overview
- Key modules and their purpose
- Coding standards and conventions
- Anti-patterns specific to your codebase

See the Mira project's own [CLAUDE.md](../CLAUDE.md) for an example of a complete file.
