# experts

Evolutionary expert system that learns and adapts over time. Provides higher-level orchestration for expert consultations including complexity assessment and collaboration mode selection.

## Key Types

- `ExpertRole` - Role enum: Architect, CodeReviewer, Security, PlanReviewer, ScopeAnalyst
- `ComplexityAssessment` - Analyzes problem complexity to determine approach
- `CollaborationMode` - Parallel, Sequential, Hierarchical, or Single expert mode

## Sub-modules

| Module | Purpose |
|--------|---------|
| `adaptation` | Expert behavior adaptation based on feedback |
| `collaboration` | Multi-expert collaboration mode selection |
| `consultation` | Consultation orchestration |
| `patterns` | Pattern learning from expert interactions |

## Relationship to tools/core/experts

This module provides the evolutionary/adaptive layer, while `tools/core/experts` handles the direct tool implementation and agentic execution loop.
