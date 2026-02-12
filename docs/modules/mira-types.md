<!-- docs/modules/mira-types.md -->
# mira-types

Shared data contracts between the Mira server and clients.

## Overview

Defines the core domain model types used across crate boundaries. Designed to work in both native and WASM builds with no native-only dependencies. Contains only data types, serde derives, and tests.

## Key Types

- `ProjectContext` -- Links a filesystem path to a database entity (id, path, name). Required for almost all operations to scope data to the correct workspace.
- `MemoryFact` -- A semantic unit of knowledge with evidence-based lifecycle. Starts as `"candidate"` with initial confidence, gains confidence through reinforcement across sessions. Supports scoping (`personal`, `project`, `team`) and classification (`general`, `preference`, `decision`, `context`).

## Architecture Notes

All types derive `Serialize`/`Deserialize` for JSON transport. `MemoryFact` uses serde defaults for backwards compatibility -- missing fields like `session_count`, `status`, and `scope` get sensible defaults when deserializing older data.
