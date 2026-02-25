<!-- docs/modules/mira-server/background/code_health.md -->
# background/code_health

Background worker for detecting code health issues using pattern-based analysis.

## Detection Methods

1. **Pattern-based** - TODOs, unwraps, unused functions
2. **Cargo warnings** - Runs `cargo check` and parses warnings

## Key Functions

### Background entry points (called by slow lane)

- `process_health_fast_scans()` - Fast pattern-based detection pass

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
