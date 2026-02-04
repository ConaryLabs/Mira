# tools/core/experts

Agentic expert sub-agents with tool access and LLM-powered reasoning. Implements the unified `expert` tool (`action="consult"` and `action="configure"`).

## Constants

| Constant | Value |
|----------|-------|
| `MAX_ITERATIONS` | 100 |
| `EXPERT_TIMEOUT` | 10 minutes |
| `LLM_CALL_TIMEOUT` | 6 minutes |
| `PARALLEL_EXPERT_TIMEOUT` | 15 minutes |
| `MAX_CONCURRENT_EXPERTS` | 3 |

## Key Exports

- `consult_expert()` — Single expert consultation (agentic loop)
- `consult_experts()` — Multi-expert consultation via council or parallel fallback
- `run_council()` — Coordinator-driven council pipeline
- `configure_expert()` — Expert configuration management
- `ExpertRole` — Role enum
- `FindingsStore` — Structured finding collection for council mode
- `ReasoningStrategy` — Single or Decoupled LLM client pairing

## Sub-modules

| Module | Purpose |
|--------|---------|
| `council` | Council pipeline: Plan → Execute → Review → Delta → Synthesize |
| `execution` | Single expert agentic loop and parallel orchestration |
| `strategy` | `ReasoningStrategy` enum — Single (one model) or Decoupled (chat + reasoner) |
| `plan` | Research plan and task structures for council mode |
| `role` | `ExpertRole` enum and role metadata |
| `prompts` | System prompts with stakes framing, accountability, and self-checks |
| `findings` | `FindingsStore` and `CouncilFinding` for structured expert output |
| `config` | Expert configuration CRUD |
| `context` | Context preparation and learned pattern injection |
| `tools` | Tools available to expert sub-agents (search, read, symbols, callers, recall, web, MCP) |
