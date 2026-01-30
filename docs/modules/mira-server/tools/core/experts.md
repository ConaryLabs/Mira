# tools/core/experts

Agentic expert sub-agents with tool access and LLM-powered reasoning. Implements the `consult_experts` and `configure_expert` tools.

## Constants

| Constant | Value |
|----------|-------|
| `MAX_ITERATIONS` | 100 |
| `EXPERT_TIMEOUT` | 10 minutes |
| `LLM_CALL_TIMEOUT` | 6 minutes |

## Key Exports

- `consult_expert()` - Single expert consultation
- `consult_experts()` - Parallel multi-expert consultation (max concurrency: 3)
- `configure_expert()` - Expert configuration management
- `ExpertRole` - Role enum
- `ParsedFinding` - Parsed finding from expert output

## Sub-modules

| Module | Purpose |
|--------|---------|
| `execution` | Expert execution loop and parallel orchestration |
| `role` | `ExpertRole` enum and role metadata |
| `prompts` | System prompts for each expert role |
| `findings` | Parsing and storing findings from expert output |
| `config` | Expert configuration CRUD |
| `context` | Context preparation for expert consultations |
| `tools` | Tools available to expert sub-agents |
