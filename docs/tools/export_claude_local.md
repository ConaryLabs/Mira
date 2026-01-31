# export_claude_local

Export Mira memories to `CLAUDE.local.md` for persistence across Claude Code sessions. Uses hotness-ranked, budget-aware packing to select the most valuable memories.

## Usage

```json
{
  "name": "export_claude_local",
  "arguments": {}
}
```

## Parameters

None. Operates on the current active project.

## Returns

- **Exported**: `Exported 12 memories to /home/user/myproject/CLAUDE.local.md`
- **Nothing to export**: `No memories to export (or all memories are low-confidence).`

## Behavior

### Hotness Ranking

Memories are ranked by a hotness score computed in SQL:

```
hotness = session_count * MAX(confidence, 0.5) * status_mult * category_mult / recency_penalty
```

| Factor | Values |
|--------|--------|
| `status_mult` | confirmed = 1.5, candidate = 1.0 |
| `category_mult` | preference = 1.4, decision = 1.3, pattern/convention = 1.1, context = 1.0, general = 0.9 |
| `recency_penalty` | `1.0 + (days_since_update / 90.0)` — gentle linear decay |

Memories with confidence below 0.5 are excluded. Health and persona fact types are filtered out.

### Budget-Aware Packing

Up to 200 ranked memories are fetched, then packed into an **8192 byte budget** using a greedy knapsack approach:

- Memories are iterated in hotness order (highest first)
- Individual memories are capped at **500 bytes** (truncated with "..." if longer)
- Section headers (`## Preferences`, etc.) are only counted against the budget when a section gets its first entry
- If a memory doesn't fit, it's skipped (up to 10 consecutive skips before stopping)

### Sections

Memories are grouped by type in fixed order:

1. **Preferences** — `preference` fact type or category
2. **Decisions** — `decision` fact type or category
3. **Patterns** — `pattern`, `convention` fact type or category
4. **General** — everything else

### Auto-Export on Session Close

The **Stop hook** automatically exports memories to `CLAUDE.local.md` when a Claude Code session ends. This means the file stays up-to-date without manual tool calls.

### Import on Session Start

When a project initializes, `CLAUDE.local.md` is parsed and entries are imported as confirmed memories (confidence 0.9). Duplicates are detected via normalized whitespace comparison and skipped.

## Output File Format

```markdown
# CLAUDE.local.md

<!-- Auto-generated from Mira memories. Manual edits will be imported back. -->

## Preferences
- Use tabs for indentation

## Decisions
- Using SQLite for persistence

## Patterns
- Builder pattern for Config struct
```

## Errors

- **"No active project. Call session_start first."**: Requires an active project context.
- **Database errors**: Failed to query memories.

## See Also

- **remember**: Store memories that can be exported
- **recall**: Search memories
- **project**: Initialize project context (imports CLAUDE.local.md on start)
