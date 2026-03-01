<!-- docs/tools/team.md -->
# Team

> **This entire tool is CLI-only.** All actions are available via `mira tool team '<json>'` but are not exposed as MCP tools.

Team intelligence for Claude Code Agent Teams. Provides visibility into team activity, file conflicts, and knowledge distillation.

Requires an active Agent Teams session. Returns an informational message (not an error) when no team is active.

## Actions

### status

Get team overview: active members, files they have modified, and file conflicts (multiple teammates editing the same file).

**Parameters:**
- `action` (string, required) - `"status"`

**Returns:** Team name, team ID, active member count, member details (name, role, status, last heartbeat, files), and file conflicts (file path with list of editors).

### review

Review a teammate's modified files.

**Parameters:**
- `action` (string, required) - `"review"`
- `teammate` (string, optional) - Teammate name to review (defaults to self)

**Returns:** Member name, list of modified files, and file count.

**Note:** If the specified teammate is not found, the error includes a list of active members.

### distill

Extract key findings and decisions from team work into team-scoped memories for future recall.

**Parameters:**
- `action` (string, required) - `"distill"`

**Returns:** Team name, findings count, memories processed, files touched, and distilled findings with categories and source counts.

**Note:** Requires sufficient team activity (at least 2 memories or file modifications) to produce results.

## Examples

```json
{"action": "status"}
```

```json
{"action": "review", "teammate": "researcher"}
```

```json
{"action": "distill"}
```

## See Also

- [memory](./memory.md) - Team-scoped memories via `scope="team"`
