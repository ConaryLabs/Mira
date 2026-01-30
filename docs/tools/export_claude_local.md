# export_claude_local

Export Mira memories to `CLAUDE.local.md` for persistence across Claude Code sessions. Writes high-confidence memories as a structured markdown file in the project root.

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

- Queries memories with confidence >= 0.7
- Groups memories by type: Preferences, Decisions, Patterns, General
- Writes to `{project_path}/CLAUDE.local.md`
- Overwrites the file on each export
- Includes an auto-generated header noting that manual edits will be imported back on session start

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

## Examples

**Example 1: Export memories**
```json
{
  "name": "export_claude_local",
  "arguments": {}
}
```

## Errors

- **"No active project. Call session_start first."**: Requires an active project context.
- **Database errors**: Failed to query memories.

## See Also

- **remember**: Store memories that can be exported
- **recall**: Search memories
- **project**: Initialize project context (imports CLAUDE.local.md on start)
