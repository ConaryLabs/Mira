# usage

Query LLM usage and cost analytics. Provides totals, grouped statistics, and recent usage records for tracking token consumption, API costs, and performance metrics across providers, models, and expert roles.

## Usage

```json
{
  "name": "usage",
  "arguments": {
    "action": "summary",
    "since_days": 30
  }
}
```

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| action | String | Yes | Action to perform: `summary`, `stats`, or `list` |
| group_by | String | No | Grouping dimension for `stats` action: `role`, `provider`, `model`, or `provider_model` (default: `role`) |
| since_days | Integer | No | Filter to last N days (default: 30) |
| limit | Integer | No | Maximum results for `list` action (default: 50) |

### Actions

| Action | Description |
|--------|-------------|
| `summary` | Aggregate totals: request count, token breakdown, estimated cost, average duration |
| `stats` | Usage statistics grouped by a dimension, displayed as a formatted table |
| `list` | Recent usage records listed by role with request counts, tokens, and costs |

## Returns

### `summary`

Formatted text with aggregate totals for the time period:

```
LLM Usage Summary (last 30 days)

Requests: 150
Total tokens: 45000 (30000 prompt + 15000 completion)
Estimated cost: $0.2500
Avg duration: 1234ms
```

### `stats`

Formatted table grouped by the specified dimension:

```
LLM Usage by role (last 30 days)

ROLE                           REQUESTS       TOKENS       COST
-----------------------------------------------------------------
code_reviewer                       100        30000  $   0.1500
architect                            50        15000  $   0.1000
-----------------------------------------------------------------
TOTAL                               150        45000  $   0.2500
```

### `list`

Bullet list of recent usage by role:

```
Recent LLM Usage by Role (last 30 days, limit 50)

- code_reviewer: 100 requests, 30000 tokens, $0.1500
- architect: 50 requests, 15000 tokens, $0.1000
```

## Examples

**Example 1: Get usage summary for the last week**
```json
{
  "name": "usage",
  "arguments": {
    "action": "summary",
    "since_days": 7
  }
}
```

**Example 2: View costs broken down by provider and model**
```json
{
  "name": "usage",
  "arguments": {
    "action": "stats",
    "group_by": "provider_model",
    "since_days": 30
  }
}
```

**Example 3: List recent usage by role**
```json
{
  "name": "usage",
  "arguments": {
    "action": "list",
    "since_days": 90,
    "limit": 100
  }
}
```

## Errors

- **Database error**: Failed to query the `llm_usage` table.
- **Invalid action**: The `action` parameter must be one of `summary`, `stats`, or `list`.
- **Invalid group_by**: For the `stats` action, `group_by` must be `role`, `provider`, `model`, or `provider_model`.

## See Also

- **consult_experts**: Expert consultations that generate LLM usage records
- **configure_expert**: Configure expert providers and models that affect usage
- **session_history**: View session activity including tool call history
