# experts (deprecated)

There is no top-level `experts` module in the current `mira-server` crate. The active expert system lives under `tools/core/experts/`.

## Current Implementation

See `docs/modules/mira-server/tools/core/experts.md` for the live expert architecture, including council mode, agentic loops, findings storage, and reasoning strategy.

## Historical Note

Older versions had a separate expert orchestration module with collaboration modes and adaptation logic. That code has been retired in favor of the unified `tools/core/experts` implementation.
