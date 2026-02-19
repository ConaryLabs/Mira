---
name: recap
description: This skill should be used when the user asks "what were we working on", "session recap", "remind me of context", "catch me up", "restore context", "previous session", or starts a new session needing prior context.
---

# Session Recap

## Current Context (Live)

!`mira tool session '{"action":"recap"}'`

## Instructions

Present the recap above in a clear, organized format:
- **Preferences**: User coding style, tool preferences
- **Recent Context**: What was worked on recently
- **Active Goals**: In-progress goals with milestones

Highlight any blocked or high-priority items. If the recap above is empty, suggest using `memory(action="remember")` to store context for future sessions.

## When to Use

- Starting a new session
- Resuming after a break
- When context about previous work is needed
- User asks "what were we working on?"
