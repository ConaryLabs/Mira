# cross_project

Privacy-preserving pattern sharing across projects. Enables learning from patterns in one project and applying them in others while protecting sensitive information.

## Key Types

- `CrossPatternType` - Pattern categories: `FileSequence`, `ToolChain`, `ProblemPattern`, etc.
- `SharingDirection` - Import/export control
- `CrossProjectConfig` - Privacy settings including k-anonymity and differential privacy epsilon

## Sub-modules

| Module | Purpose |
|--------|---------|
| `anonymizer` | Pattern anonymization for privacy |
| `preferences` | Sharing preference management |
| `storage` | Pattern storage and retrieval |

## Privacy Features

- k-anonymity for pattern generalization
- Differential privacy with configurable epsilon
- Per-project import/export controls
