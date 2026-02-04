# experts

Legacy expert orchestration module. Contains complexity assessment and collaboration mode abstractions that were superseded by the council architecture in `tools/core/experts`.

## Key Types

- `ExpertRole` — Role enum: Architect, CodeReviewer, Security, PlanReviewer, ScopeAnalyst
- `ComplexityAssessment` — Analyzes problem complexity to determine approach
- `CollaborationMode` — Parallel, Sequential, Hierarchical, or Single expert mode (not actively used by `expert(action="consult")`)

## Sub-modules

| Module | Purpose |
|--------|---------|
| `adaptation` | Expert behavior adaptation based on feedback |
| `collaboration` | Multi-expert collaboration mode selection |
| `consultation` | Consultation orchestration |
| `patterns` | Pattern learning from expert interactions |

## Relationship to tools/core/experts

The active expert implementation lives in `tools/core/experts/`, which provides:
- **Council mode** — Coordinator-driven multi-expert consultation with planning, review, and delta rounds
- **Single expert mode** — Agentic tool-using loop
- **ReasoningStrategy** — Decoupled chat/reasoner client pairing
- **FindingsStore** — Structured finding collection and review

This module provides auxiliary evolutionary/adaptive abstractions.
