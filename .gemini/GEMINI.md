# Mira Project Mandate

This project is deeply integrated with Mira (Memory and Intelligence Layer for AI Agents). Always prioritize Mira's tools and context for all development tasks.

## Commands
- `/mira:start`: Initialize the Mira session (do this first in every session).
- `/mira:recap`: Restore context from previous sessions (preferences, memories, goals).
- `/mira:status`: Check project health, index stats, and Mira configuration.
- `/mira:search <query>`: Semantic code search — find code by meaning.
- `/mira:experts [context]`: Consult the AI expert team (architect, code-reviewer, security).
- `/mira:insights`: Surface background analysis patterns and predicted next steps.
- `/mira:goals`: Track and manage cross-session objectives with milestones.
- `/mira:docs`: Check for stale documentation needing updates.
- `/mira:full-cycle`: End-to-end expert review with implementation and QA.
- `/mira:qa-hardening`: Production readiness and hardening pass.
- `/mira:refactor`: Safe code restructuring with architect validation.
- `/mira:diff`: Semantic analysis of git changes with impact assessment.

## MCP Tools
Use the `mira__` prefixed tools for deep structural and semantic analysis:
- `mira__code`: Call graph tracing, symbol extraction, and semantic search.
- `mira__memory`: Persistent cross-session project memory.
- `mira__documentation`: Manage documentation lifecycle and staleness.
- `mira__diff`: Semantic change intelligence.
- `mira__goal`: Objective tracking and progress management.

## Mandatory Workflow
1. **Startup**: Run `/mira:start` and `/mira:recap` at the beginning of every session to align with the project's current state and your personal coding preferences.
2. **Analysis**: Use `mira__code` (search, callers, callees) to understand code impact and dependencies before making changes.
3. **Execution**: After significant changes, run `/mira:diff` or `/mira:experts` to validate the work against architectural and security standards.
4. **Documentation**: Run `/mira:docs` to verify if your changes require updates to existing documentation.
5. **Memory Cross-Pollination**: 
   - Use `mira__memory(action="remember", content="...")` to store project-specific context, architectural decisions, and bug-fix rationales.
   - Use Gemini CLI's `save_memory` for global user preferences and universal facts.
   - **Rule**: If a fact is relevant to this project's history or codebase, it MUST be in Mira's memory.

## Development Standards
- Adhere to the patterns detected by `/mira:insights`.
- Maintain the architectural integrity identified in `/mira:status`.
- Do not bypass Mira's expert validation for critical security or infrastructure changes.
