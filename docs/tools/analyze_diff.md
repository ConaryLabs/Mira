# analyze_diff

Analyze git diffs semantically. Identifies change types, impact, and risks using LLM-powered analysis with heuristic fallback.

## Usage

```json
{
  "name": "analyze_diff",
  "arguments": {
    "from_ref": "HEAD~1",
    "to_ref": "HEAD"
  }
}
```

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| from_ref | String | No | Starting git ref (commit, branch, tag). Default: HEAD~1, or analyzes staged/working changes if present |
| to_ref | String | No | Ending git ref. Default: HEAD |
| include_impact | Boolean | No | Include impact analysis via call graph traversal (default: true) |

## What It Does

1. **Change Classification** — Categorizes each change (NewFunction, ModifiedFunction, DeletedFunction, etc.)
2. **Impact Analysis** — Traverses the call graph to find affected callers
3. **Risk Assessment** — Flags breaking changes, security-relevant modifications
4. **Caching** — Results are stored in `diff_analyses` table for reuse

## Graceful Degradation

| With LLM | Without LLM |
|----------|-------------|
| Semantic change classification via DeepSeek Reasoner | Heuristic: regex-based function and security detection |

## Examples

**Analyze the last commit:**
```json
{ "from_ref": "HEAD~1", "to_ref": "HEAD" }
```

**Analyze staged changes:**
```json
{}
```

**Compare branches without impact analysis:**
```json
{ "from_ref": "main", "to_ref": "feature-branch", "include_impact": false }
```

## See Also

- [**code**](./code.md): Call graph queries used for impact analysis
- [**finding**](./finding.md): Review findings generated from analysis
- [**index**](./index.md): Code index powers the call graph traversal
