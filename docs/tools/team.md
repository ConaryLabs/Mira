# team

Manage teams for shared memory. Teams allow multiple users to share memories scoped to the team, enabling collaborative context across sessions and users.

## Usage

```json
{
  "name": "team",
  "arguments": {
    "action": "create",
    "name": "backend-team",
    "description": "Backend engineering team"
  }
}
```

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| action | String | Yes | Action to perform: `create`, `invite`, `add`, `remove`, `list`, or `members` |
| team_id | Integer | Conditional | Team ID (required for `invite`, `add`, `remove`, `members`) |
| name | String | Conditional | Team name (required for `create`) |
| description | String | No | Team description (for `create`) |
| user_identity | String | Conditional | User identity to invite or remove (required for `invite`, `add`, `remove`) |
| role | String | No | Role for invited user: `member` or `admin` (default: `member`) |

### Actions

| Action | Description | Required Params |
|--------|-------------|-----------------|
| `create` | Create a new team (you become admin) | `action`, `name` |
| `invite` | Invite a user to a team | `action`, `team_id`, `user_identity` |
| `add` | Alias for `invite` | `action`, `team_id`, `user_identity` |
| `remove` | Remove a user from a team | `action`, `team_id`, `user_identity` |
| `list` | List teams you belong to | `action` |
| `members` | List members of a team | `action`, `team_id` |

## Returns

### `create`

```
Created team 'backend-team' (id: 1). You are now the admin.
```

### `invite` / `add`

```
Added 'alice@example.com' to team 'backend-team' as member
```

### `remove`

```
Removed 'alice@example.com' from team 'backend-team'
```

### `list`

```
Your teams (2):
  [1] backend-team - Backend engineering team
  [2] frontend-team
```

Or: `You are not a member of any teams.`

### `members`

```
Members of 'backend-team' (3):
  bob@example.com (admin) - joined 2025-01-10
  alice@example.com (member) - joined 2025-01-12
  carol@example.com (member) - joined 2025-01-15
```

## Authorization

| Action | Who Can Do It |
|--------|---------------|
| `create` | Any identified user |
| `invite` / `add` | Team admins only |
| `remove` | Team admins, or a user removing themselves |
| `list` | Shows only your own teams |
| `members` | Team members only |

## Examples

**Example 1: Create a team and invite a member**
```json
{
  "name": "team",
  "arguments": { "action": "create", "name": "platform", "description": "Platform team" }
}
```
```json
{
  "name": "team",
  "arguments": { "action": "invite", "team_id": 1, "user_identity": "alice@example.com", "role": "member" }
}
```

**Example 2: List your teams**
```json
{
  "name": "team",
  "arguments": { "action": "list" }
}
```

**Example 3: View team members**
```json
{
  "name": "team",
  "arguments": { "action": "members", "team_id": 1 }
}
```

## Errors

- **"Cannot create team: user identity not available"**: Identity must be established before creating teams.
- **"Team '{name}' already exists"**: Team names must be unique.
- **"Team {id} not found"**: The specified team ID does not exist.
- **"Only team admins can invite members"**: You must be an admin to invite users.
- **"Only team admins can remove members"**: You must be an admin to remove others (you can always remove yourself).
- **"You must be a team member to view the member list"**: Only members can see the member list.

## See Also

- **remember**: Store memories with `scope: "team"` to share with team members
- **recall**: Search memories includes team-scoped memories for teams you belong to
