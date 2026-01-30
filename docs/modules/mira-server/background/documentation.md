# background/documentation

Background worker for tracking documentation gaps and staleness. Detects when source code changes make existing documentation outdated.

## Key Functions

- `process_documentation()` - Main entry point for documentation scanning
- `calculate_source_signature_hash()` - Detect API changes in source files
- `needs_documentation_scan()` - Check if rescan is needed
- `mark_documentation_scanned_sync()` - Mark scan complete

## Sub-modules

| Module | Purpose |
|--------|---------|
| `detection` | Documentation gap scanning |
| `inventory` | Documentation tracking and staleness detection |

## Staleness Detection

Uses source file signature hashing to detect when a documented file's public API has changed since documentation was last written.
