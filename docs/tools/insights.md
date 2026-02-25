<!-- docs/tools/insights.md -->
# Insights

Background analysis digest — surfaces pondering results, proactive predictions, and documentation gaps as a unified Project Health Dashboard.

> **MCP actions:** `insights`, `dismiss_insight`

## Actions

### insights

Query the unified insights digest, formatted as a categorized Health Dashboard.

**Parameters:**
- `action` (string, required) - `"insights"`
- `insight_source` (string, optional) - Filter by source: `pondering`, `proactive`, `doc_gap`
- `min_confidence` (float, optional) - Minimum confidence threshold 0.0–1.0 (default: 0.5)
- `since_days` (integer, optional) - Look back period in days (default: 30)
- `limit` (integer, optional) - Max results (default: 20)

**Returns:** Formatted Health Dashboard with overall score, tier breakdown, and categorized insights. Each insight has a priority indicator, age label, optional evidence, and an `insight_id` for dismissal.

**Priority indicators:**

| Indicator | Threshold |
|-----------|-----------|
| `[!!]` | priority_score ≥ 0.75 — Attention Required |
| `[!]` | priority_score ≥ 0.50 — Notable |
| `[ ]` | priority_score < 0.50 — Low priority |

**Health score tiers:**

| Score | Grade |
|-------|-------|
| 0–20 | A (Healthy) |
| 21–40 | B (Moderate) |
| 41–60 | C (Needs Work) |
| 61–80 | D (Poor) |
| 81–100 | F (Critical) |

**Insight sources:**

| Source | Origin |
|--------|--------|
| `pondering` | Background LLM analysis of code patterns |
| `proactive` | Predictive suggestions based on session context |
| `doc_gap` | Missing or stale documentation detected by the cartographer |

### dismiss_insight

Remove a resolved insight so it no longer appears in future queries.

**Parameters:**
- `action` (string, required) - `"dismiss_insight"`
- `insight_id` (integer, required) - Row ID of the insight to dismiss (from `insights` output)
- `insight_source` (string, required) - Source table: `"pondering"` or `"doc_gap"`

**Returns:** Confirmation message or "not found" if the insight doesn't exist or was already dismissed.

> `insight_source` is required to prevent cross-table ID collisions between `behavior_patterns` (pondering) and `documentation_tasks` (doc_gap).

## Examples

```json
{"action": "insights"}
```

```json
{"action": "insights", "insight_source": "pondering", "min_confidence": 0.7}
```

```json
{"action": "insights", "insight_source": "doc_gap", "since_days": 7}
```

```json
{"action": "dismiss_insight", "insight_id": 42, "insight_source": "pondering"}
```

```json
{"action": "dismiss_insight", "insight_id": 221, "insight_source": "doc_gap"}
```

## Generating Insights

If no insights appear, run a health scan first:

1. Index the project: `index(action="project")`
2. Run the health scan (CLI): `mira tool index '{"action":"health"}'`
3. Query insights again

## Errors

- **"No active project"** - Both actions require an active project
- **"insight_id is required"** - `dismiss_insight` requires an `insight_id`
- **"insight_source is required"** - `dismiss_insight` requires `"pondering"` or `"doc_gap"`
- **"Insight N (source) not found or already dismissed"** - Invalid ID or already dismissed

## See Also

- [index](./index.md) - Run `health` action to populate insights
- [session](./session.md) - `recap` surfaces high-priority insights in the session summary
- [documentation](./documentation.md) - Doc gap insights link to this tool
