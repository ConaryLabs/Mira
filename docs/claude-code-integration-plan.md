# docs/claude-code-integration-plan.md
# Claude Code Deep Integration Plan

Based on analysis of Claude Code's internal system prompts from the piebald-ai/claude-code-system-prompts repository.

## Overview

This plan tightens Mira's integration with Claude Code by:
1. Matching Claude Code's expected patterns and formats
2. Bridging ephemeral (session) and persistent (cross-session) systems
3. Injecting memory context where Claude Code has gaps
4. Synchronizing competing systems (TodoWrite vs Mira tasks, /remember vs Mira memory)

## Goals

### Goal 1: CLAUDE.md Tool Description Overhaul

Reformat Mira's tool documentation in CLAUDE.md to match Claude Code's native tool description style, improving model compliance.

**Key changes:**
- Add `<example>` tags with `<reasoning>` explanations
- Add "When to use" and "When NOT to use" sections
- Add contrast tables (Wrong vs Right)
- Mirror the style of tool-description-todowrite.md and tool-description-task.md

**Files:**
- `/home/peter/Mira/CLAUDE.md`

### Goal 2: Session Notes Integration

Read and merge Claude Code's session memory files into Mira's session context, preserving information that would otherwise be lost.

**Key insight:** Claude Code stores session notes at:
`~/.claude/projects/{sanitized-project-path}/{session-id}/session-memory/summary.md`

**Implementation:**
- Add function to discover and read Claude Code session notes
- Parse the template structure (Current State, Task specification, etc.)
- Merge into `get_session_recap` output or store as Mira memories
- Consider watching for changes during active sessions

**Files:**
- `crates/mira-server/src/tools/session.rs` (or new module)
- `crates/mira-server/src/mcp/tools.rs`

### Goal 3: CLAUDE.local.md Bridge

Enable bidirectional sync between Mira's memory system and Claude Code's CLAUDE.local.md file.

**Key insight:** Claude Code's `/remember` skill writes patterns to CLAUDE.local.md. Mira's `remember()` stores in SQLite. These should be unified.

**Implementation:**
- On session_start: parse CLAUDE.local.md, import as Mira memories (if not already present)
- New tool or flag: export high-confidence Mira memories to CLAUDE.local.md
- Deduplicate by content similarity
- Respect CLAUDE.local.md format (markdown sections with bullet points)

**Files:**
- `crates/mira-server/src/tools/memory.rs`
- `crates/mira-server/src/mcp/tools.rs`

### Goal 4: TodoWrite Synchronization

Bridge the gap between Claude Code's ephemeral TodoWrite and Mira's persistent tasks.

**Options:**
1. **Session-end sync**: On session end, persist incomplete TodoWrite items to Mira tasks
2. **Real-time sync**: When creating a Mira task, also write to TodoWrite for UI visibility
3. **CLAUDE.md redirect**: Instruct model to prefer Mira tasks over TodoWrite

**Chosen approach:** Option 3 (CLAUDE.md redirect) + future Option 1 if we can detect session end

**Implementation:**
- Update CLAUDE.md to clearly redirect task management to Mira
- Add examples showing Mira task usage patterns
- Consider hook for session-end detection (future)

**Files:**
- `/home/peter/Mira/CLAUDE.md`

### Goal 5: Evidence-Based Memory System

Implement Claude Code's "2+ sessions" evidence threshold for auto-memories.

**Key insight:** Claude Code's `/remember` skill requires patterns to appear in 2+ sessions before storing. This prevents one-off observations from polluting memory.

**Implementation:**
- Add `session_count` field to memories (or track separately)
- New category: "candidate" for single-session observations
- Promote "candidate" to "general" after 2+ session appearances
- Add `confidence` scoring based on recurrence

**Files:**
- `crates/mira-server/src/db/schema.rs` (migration)
- `crates/mira-server/src/db/memory.rs`
- `crates/mira-server/src/tools/memory.rs`

### Goal 6: Sub-agent Memory Injection

Automatically inject relevant Mira context into Claude Code sub-agent prompts.

**Key insight:** Sub-agents (Explore, Plan, etc.) don't see MCP tool results or memories unless explicitly passed. They only see the conversation history up to the Task call.

**Implementation:**
- Add CLAUDE.md instruction: "Before spawning sub-agents, recall relevant context and include in prompt"
- Provide example patterns for different agent types
- Consider: format recalls as a preamble the model should include

**Files:**
- `/home/peter/Mira/CLAUDE.md`

### Goal 7: Pre-summarization Context Preservation (Future/Exploratory)

Investigate if Claude Code exposes hooks before conversation summarization, allowing Mira to persist critical context.

**Key insight:** Summarization loses nuance. If we can detect "about to summarize" events, we can `remember()` key details.

**Implementation:**
- Research: Check if Claude Code hooks support pre-summarization events
- If available: Add hook that calls `remember()` with current context
- If not: Document as future enhancement request

**Status:** Exploratory - may not be possible with current Claude Code architecture

## Priority Order

1. **Goal 1** (CLAUDE.md overhaul) - Immediate, high impact, low effort
2. **Goal 4** (TodoWrite redirect in CLAUDE.md) - Can do with Goal 1
3. **Goal 6** (Sub-agent injection in CLAUDE.md) - Can do with Goal 1
4. **Goal 3** (CLAUDE.local.md bridge) - Medium effort, high value
5. **Goal 2** (Session notes integration) - Medium effort, high value
6. **Goal 5** (Evidence-based memory) - Medium effort, architectural improvement
7. **Goal 7** (Pre-summarization) - Exploratory, dependent on Claude Code capabilities

## Success Metrics

- Model follows Mira tool instructions more consistently
- Cross-session context preservation improves
- Less "forgotten" decisions across sessions
- Reduced duplication between Mira memory and CLAUDE.local.md
- Sub-agents have access to relevant project context
