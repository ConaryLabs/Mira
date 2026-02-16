<!-- docs/modules/mira-server/background/documentation.md -->
# background/documentation

Background worker for tracking documentation gaps and staleness. Detects when source code changes make existing documentation outdated.

## Key Functions

- `process_documentation()` - Main entry point for documentation scanning
- `calculate_source_signature_hash()` - Detect API changes in source files
- `hash_normalized_signatures()` - Hash normalized function signatures
- `file_checksum()` - Compute file checksum for change detection
- `read_file_content()` - Read file content for analysis
- `needs_documentation_scan(conn, project_id, project_path)` - Check if rescan is needed
- `mark_documentation_scanned_sync(conn, project_id, project_path)` - Mark scan complete
- `clear_documentation_scan_marker_sync()` - Clear scan marker to force rescan
- `get_git_head()` / `is_ancestor()` - Git helpers (re-exported from git module)
- `DOC_SCAN_MARKER_KEY` - Public constant for scan marker

## Key Types

- `CodeSymbol` - Represents a code symbol for documentation tracking

## Sub-modules

| Module | Purpose |
|--------|---------|
| `detection` | Documentation gap scanning |
| `inventory` | Documentation tracking and staleness detection |

## Staleness Detection

Uses source file signature hashing to detect when a documented file's public API has changed since documentation was last written.
