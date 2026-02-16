<!-- docs/modules/mira-server/background/code_health.md -->
# background/code_health

Background worker for detecting code health issues. Combines concrete signal detection with LLM-powered analysis.

## Detection Methods

1. **Pattern-based** - TODOs, unwraps, unused functions
2. **Cargo warnings** - Runs `cargo check` and parses warnings
3. **LLM analysis** - Complexity assessment, error handling quality

## Key Functions

### Background entry points (called by slow lane)

- `process_health_fast_scans()` - Fast pattern-based detection pass
- `process_health_llm_complexity()` - LLM-powered complexity analysis
- `process_health_llm_error_quality()` - LLM-powered error handling quality analysis
- `process_health_module_analysis()` - LLM-powered module-level analysis

### On-demand (called by MCP tool)

- `scan_project_health_full()` - Orchestrates full health scan for MCP tool use

### Utilities

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
