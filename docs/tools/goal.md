# goal

Manage goals and milestones. Goals persist across sessions for tracking multi-session objectives. Supports bulk operations.

## Usage

```json
{
  "name": "goal",
  "arguments": {
    "action": "create",
    "title": "v2.0 Release",
    "description": "Ship new features",
    "priority": "high"
  }
}
```

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| action | String | Yes | `create`, `bulk_create`, `list`, `get`, `update`, `delete`, `add_milestone`, `complete_milestone`, `delete_milestone`, or `progress` |
| goal_id | Integer | Conditional | Goal ID (required for `get`, `update`, `delete`, `add_milestone`, `progress`) |
| title | String | Conditional | Goal title (required for `create`) |
| description | String | No | Goal description |
| status | String | No | `planning`, `in_progress`, `blocked`, `completed`, or `abandoned` |
| priority | String | No | `low`, `medium`, `high`, or `critical` |
| progress_percent | Integer | No | Manual progress override (0-100) |
| include_finished | Boolean | No | Include completed/abandoned goals in `list` |
| milestone_id | Integer | Conditional | Milestone ID (for `complete_milestone`, `delete_milestone`) |
| milestone_title | String | Conditional | Milestone title (for `add_milestone`) |
| weight | Integer | No | Milestone weight (for `add_milestone`, default: 1) |
| limit | Integer | No | Max results for `list` |
| goals | String | Conditional | JSON array of goals for `bulk_create`: `[{title, description?, priority?}, ...]` |

## Actions

### `create` — Create a goal

```json
{ "action": "create", "title": "Implement auth system", "priority": "high" }
```

### `bulk_create` — Create multiple goals

```json
{ "action": "bulk_create", "goals": "[{\"title\": \"Fix login\", \"priority\": \"high\"}, {\"title\": \"Add tests\"}]" }
```

### `list` — List goals

```json
{ "action": "list" }
{ "action": "list", "include_finished": true }
```

### `get` — Get goal details

```json
{ "action": "get", "goal_id": 1 }
```

### `update` — Update a goal

```json
{ "action": "update", "goal_id": 1, "status": "in_progress" }
```

### `delete` — Delete a goal

```json
{ "action": "delete", "goal_id": 1 }
```

### `add_milestone` — Add milestone to a goal

```json
{ "action": "add_milestone", "goal_id": 1, "milestone_title": "Design API", "weight": 2 }
```

### `complete_milestone` — Mark milestone done

Automatically updates goal progress based on weighted milestones.

```json
{ "action": "complete_milestone", "milestone_id": 1 }
```

### `delete_milestone` — Remove a milestone

```json
{ "action": "delete_milestone", "milestone_id": 1 }
```

### `progress` — Update goal progress

```json
{ "action": "progress", "goal_id": 1, "progress_percent": 75 }
```

## See Also

- [**session**](./session.md): Session recap includes active goals
- [**project**](./project.md): Goals are scoped to projects
