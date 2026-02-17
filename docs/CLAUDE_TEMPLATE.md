# CLAUDE.md Template for Mira

Mira uses a modular structure to keep always-loaded context small:

| Location | Purpose | When Loaded |
|----------|---------|-------------|
| `CLAUDE.md` | Core identity, anti-patterns, build commands | Always |
| `.claude/rules/*.md` | Tool selection, memory, tasks | Always |
| `.claude/skills/*/SKILL.md` | Reference docs (Context7, tool APIs) | On-demand |

---

## Setup

Create these files in your project:

### 1. `CLAUDE.md` (root) — Always loaded

At minimum, add this to your project's `CLAUDE.md`:

```markdown
# CLAUDE.md

This project uses **Mira** for persistent memory and code intelligence.

## Session Start

Project context is **auto-initialized** from Claude Code's working directory.
For full session context, call `session(action="recap")`. Use `memory(action="recall", query="preferences")` before writing code.

## Tool Selection

STOP before using Grep or Glob. Prefer Mira tools for semantic work:
- **Code by intent** -> `code(action="search", query="...")` (not Grep)
- **File structure** -> `code(action="symbols", file_path="...")` (not grepping for definitions)
- **Call graph** -> `code(action="callers", ...)` / `code(action="callees", ...)` (not grepping function names)
- **Past decisions** -> `memory(action="recall", query="...")` before architectural changes
- **External libraries** -> Context7: `resolve-library-id` then `query-docs`

Use Grep/Glob only for **literal strings**, **exact filename patterns**, or **simple one-off searches**.

## Code Navigation Quick Reference

| Need | Tool |
|------|------|
| Search by meaning | `code(action="search", query="...")` |
| File structure | `code(action="symbols", file_path="...")` |
| What calls X? | `code(action="callers", function_name="...")` |
| What does X call? | `code(action="callees", function_name="...")` |
| Past decisions | `memory(action="recall", query="...")` |
| External library API | Context7: `resolve-library-id` -> `query-docs` |
| Literal string search | `Grep` (OK) |
| Exact filename pattern | `Glob` (OK) |
```

Then add your project-specific content: build commands, anti-patterns, architecture overview, etc.

### 2. `.claude/rules/` — Always loaded

Create these rule files for detailed guidance:

- **`tool-selection.md`** — When to use Mira vs Grep/Glob, wrong vs right table
- **`memory-system.md`** — Evidence threshold, when to remember/recall
- **`sub-agents.md`** — Recall before launching Task agents
- **`task-management.md`** — Session tasks vs cross-session goals

See Mira's own [.claude/rules/](../.claude/rules/) for examples of each.

### 3. `.claude/skills/` — Loaded on-demand

Create these skill files for reference content that only loads when relevant:

- **`context7/SKILL.md`** — Context7 workflow for external library docs
- **`tools-reference/SKILL.md`** — Mira consolidated tool API signatures

See Mira's own [.claude/skills/](../.claude/skills/) for examples.

---

## Structure Benefits

- **Before:** ~550 lines always loaded in a single CLAUDE.md
- **After:** ~80 lines in CLAUDE.md + ~170 lines in rules (always loaded) + ~80 lines in skills (on-demand)
- **Result:** ~59% reduction in always-loaded context vs monolithic approach
