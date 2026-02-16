<!-- docs/modules/mira-server/background/diff_analysis.md -->
# background/diff_analysis

Semantic diff analysis with LLM enhancement and heuristic fallback. Classifies code changes by type, assesses risk, and traces impact through the call graph.

## Overview

Analyzes git diffs between two refs (commits, branches, tags) to produce structured change summaries. When an LLM is available, changes are classified semantically (e.g., "refactor", "bug fix", "new feature"). Without an LLM, a heuristic regex-based analysis detects function changes, security-sensitive patterns, and risk flags. Results are cached in the database to avoid re-analysis.

## Key Functions

- `analyze_diff()` - Main entry point. Resolves refs, checks cache, runs LLM or heuristic analysis, builds impact graph, caches result.
- `analyze_diff_semantic()` - LLM-powered change classification
- `analyze_diff_heuristic()` - Regex-based fallback analysis
- `calculate_risk_level()` - Compute overall risk from flags and changes
- `build_impact_graph()` - Trace affected callers through the call graph
- `format_diff_analysis()` - Format analysis results for display
- `compute_historical_risk()` - Compute risk from historical change patterns
- `map_to_symbols()` - Map diff changes to known symbols

### Re-exported from `crate::git`

- `derive_stats_from_unified_diff`, `get_head_commit`, `get_staged_diff`, `get_unified_diff`, `get_working_diff`
- `parse_diff_stats`, `parse_numstat_output`, `parse_staged_stats`, `parse_working_stats`, `resolve_ref`

### Additional types (re-exported from `types`)

- `DiffStats`, `HistoricalRisk`, `MatchedPattern`

## Sub-modules

| Module | Purpose |
|--------|---------|
| `types` | Core types (`DiffAnalysisResult`, `SemanticChange`, `RiskAssessment`, `ImpactAnalysis`) |
| `heuristic` | Regex-based change detection and risk flagging |
| `llm` | LLM-powered semantic analysis |
| `impact` | Call graph traversal for impact analysis |
| `format` | Output formatting |

## Architecture Notes

Git operations are delegated to the centralized `crate::git` module and re-exported for backward compatibility. Results are cached in `diff_analyses` table with the analysis type ("commit" for LLM, "heuristic" for fallback). Heuristic-cached results are replaced when an LLM becomes available.
