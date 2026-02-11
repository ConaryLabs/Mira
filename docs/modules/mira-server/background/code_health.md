# background/code_health

Background worker for detecting code health issues. Combines concrete signal detection with LLM-powered analysis.

## Detection Methods

1. **Pattern-based** - TODOs, unwraps, unused functions
2. **Cargo warnings** - Runs `cargo check` and parses warnings
3. **LLM analysis** - Complexity assessment, error handling quality

## Key Functions

- `scan_project_health_full()` - Orchestrates full health scan
- `needs_health_scan()` - Check if a rescan is needed
- `mark_health_scan_needed_sync()` - Flag for rescan (triggered by file watcher)

## Sub-modules

| Module | Purpose |
|--------|---------|
| `detection` | Pattern-based issue detection |
| `cargo` | Cargo check integration |
| `analysis` | LLM-based code analysis |
| `conventions` | Code convention detection |
| `dependencies` | Dependency analysis |
| `patterns` | Architectural pattern detection |
| `scoring` | Tech debt scoring |
