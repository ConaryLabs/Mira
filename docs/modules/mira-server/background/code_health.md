<!-- docs/modules/mira-server/background/code_health.md -->
# background/code_health

Background worker for detecting code health issues using compiler output and call graph analysis.

## Detection Methods

1. **Cargo warnings** - Runs `cargo check --message-format=json` and stores real compiler warnings
2. **Unused functions** - Queries the code index call graph for functions with zero callers

## Key Functions

- `process_health_fast_scans()` - Background entry point (called by slow lane)
- `scan_project_health_full()` - Full health scan for MCP tool use
- `mark_health_scan_needed_sync()` - Flag for rescan (triggered by file watcher)

## Sub-modules

| Module | Purpose |
|--------|---------|
| `cargo` | Cargo check integration and warning parsing |
