<!-- docs/modules/mira-server/background/diff_analysis.md -->
# background/diff_analysis

Diff analysis with factual stats and call graph impact tracing.

## Overview

Analyzes git diffs between two refs (commits, branches, tags) to produce structured change summaries with factual statistics. Impact analysis traces affected callers through the indexed call graph. Results are cached in the database.

## Key Functions

- `analyze_diff()` - Main entry point. Resolves refs, checks cache, runs heuristic summary, builds impact graph, caches result.
- `analyze_diff_heuristic()` - Generates factual summary of changes
- `build_impact_graph()` - Trace affected callers through the call graph
- `format_diff_analysis()` - Format analysis results for display
- `map_to_symbols()` - Map diff file changes to known indexed symbols

## Sub-modules

| Module | Purpose |
|--------|---------|
| `types` | Core types (`DiffAnalysisResult`, `DiffStats`, `ImpactAnalysis`) |
| `heuristic` | Factual change summarization |
| `impact` | Call graph traversal for impact analysis |
| `format` | Output formatting |
