# team

Team intelligence for Claude Code Agent Teams. Actions: `status` (overview), `review` (teammate's work), `distill` (extract findings).

## Usage

```json
{
  "name": "team",
  "arguments": {
    "action": "status"
  }
}
```

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| action | String | Yes | `status`, `review`, or `distill` |
| teammate | String | No | Teammate name (for `review`; defaults to self) |

## Actions

### `status` — Team overview

Returns active team members, files they've modified, and potential conflicts (multiple teammates editing the same file).

```json
{ "action": "status" }
```

### `review` — Review teammate's work

Shows files modified by a specific teammate with operation details.

```json
{ "action": "review", "teammate": "researcher" }
```

### `distill` — Extract key findings

Distills key findings and decisions from team work into team-scoped memories for future recall.

```json
{ "action": "distill" }
```

## Requirements

Requires an active Claude Code Agent Teams session. Returns an error if no team is active.

## See Also

- [**session**](./session.md): Session management
- [**memory**](./memory.md): Team-scoped memories via `scope="team"`
