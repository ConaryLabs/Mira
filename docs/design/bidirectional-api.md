# Mira Plugin Architecture

## Overview

Mira becomes a Claude Code plugin that bundles an MCP server, hooks, and skills. Hooks provide the bidirectional communication channel - no separate WebSocket needed. Claude Code's hook system already supports context injection, decision control, and event handling.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Mira Plugin                               │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                      Hooks Layer                          │   │
│  │  SessionStart | UserPromptSubmit | PostToolUse | Stop    │   │
│  │       ↓              ↓                ↓           ↓       │   │
│  │  [context]      [context]        [alerts]    [continue?] │   │
│  └──────────────────────────────────────────────────────────┘   │
│         │                                                        │
│         ▼                                                        │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                    Mira Core                              │   │
│  │  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐     │   │
│  │  │ Memory  │  │  Code   │  │ Expert  │  │Proactive│     │   │
│  │  │ Manager │  │  Intel  │  │ System  │  │ Engine  │     │   │
│  │  └─────────┘  └─────────┘  └─────────┘  └─────────┘     │   │
│  └──────────────────────────────────────────────────────────┘   │
│         │                                                        │
│         ▼                                                        │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                    MCP Server                             │   │
│  │  remember | recall | search_code | consult_* | goal | ...│   │
│  └──────────────────────────────────────────────────────────┘   │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │                      Skills                               │   │
│  │  /mira:search | /mira:status | /mira:recap | /mira:goals │   │
│  └──────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
         │
         ▼
┌─────────────────┐
│   Claude Code   │
└─────────────────┘
```

## Plugin Structure

```
mira-plugin/
├── .claude-plugin/
│   └── plugin.json              # Plugin manifest
├── .mcp.json                    # Bundles Mira MCP server
├── hooks/
│   └── hooks.json               # Event handlers
├── skills/
│   ├── search/
│   │   └── SKILL.md             # /mira:search
│   ├── recap/
│   │   └── SKILL.md             # /mira:recap
│   └── goals/
│       └── SKILL.md             # /mira:goals
└── README.md
```

### plugin.json

```json
{
  "name": "mira",
  "description": "Semantic memory and code intelligence for Claude Code",
  "version": "1.0.0",
  "author": {
    "name": "Peter"
  },
  "homepage": "https://github.com/pchaganti/mira",
  "repository": "https://github.com/pchaganti/mira"
}
```

### .mcp.json (bundled MCP server)

```json
{
  "mcpServers": {
    "mira": {
      "command": "${CLAUDE_PLUGIN_ROOT}/bin/mira",
      "args": ["serve", "--mode", "plugin"],
      "env": {
        "MIRA_DB": "${HOME}/.mira/mira.db",
        "DEEPSEEK_API_KEY": "${DEEPSEEK_API_KEY}",
        "GOOGLE_API_KEY": "${GOOGLE_API_KEY}"
      }
    }
  }
}
```

### hooks/hooks.json

```json
{
  "description": "Mira bidirectional communication hooks",
  "hooks": {
    "SessionStart": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "${CLAUDE_PLUGIN_ROOT}/bin/mira hook session-start",
            "timeout": 10
          }
        ]
      }
    ],
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "${CLAUDE_PLUGIN_ROOT}/bin/mira hook user-prompt",
            "timeout": 5
          }
        ]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "Write|Edit",
        "hooks": [
          {
            "type": "command",
            "command": "${CLAUDE_PLUGIN_ROOT}/bin/mira hook post-tool",
            "timeout": 5
          }
        ]
      }
    ],
    "PreCompact": [
      {
        "matcher": "*",
        "hooks": [
          {
            "type": "command",
            "command": "${CLAUDE_PLUGIN_ROOT}/bin/mira hook pre-compact",
            "timeout": 30
          }
        ]
      }
    ],
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "${CLAUDE_PLUGIN_ROOT}/bin/mira hook stop",
            "timeout": 5
          }
        ]
      }
    ]
  }
}
```

## Bidirectional Communication via Hooks

Claude Code hooks support rich JSON output that enables bidirectional communication:

### Input: Claude Code → Mira (via stdin)

Each hook receives JSON input with event-specific data:

```json
// SessionStart
{
  "session_id": "abc123",
  "transcript_path": "~/.claude/projects/.../abc123.jsonl",
  "cwd": "/home/user/project",
  "hook_event_name": "SessionStart",
  "source": "startup",
  "model": "claude-sonnet-4-20250514"
}

// UserPromptSubmit
{
  "session_id": "abc123",
  "hook_event_name": "UserPromptSubmit",
  "prompt": "let's continue the auth work"
}

// PostToolUse
{
  "session_id": "abc123",
  "hook_event_name": "PostToolUse",
  "tool_name": "Write",
  "tool_input": {
    "file_path": "/home/user/project/src/auth.rs",
    "content": "..."
  },
  "tool_response": {
    "success": true
  }
}
```

### Output: Mira → Claude Code (via stdout JSON)

Hooks return JSON to inject context, show alerts, or control flow:

```json
// Context injection (SessionStart, UserPromptSubmit)
{
  "hookSpecificOutput": {
    "hookEventName": "UserPromptSubmit",
    "additionalContext": "Previous auth decisions:\n- JWT for tokens\n- bcrypt for passwords\n- Middleware pattern for auth checks"
  }
}

// Alternative: systemMessage for higher priority
{
  "systemMessage": "IMPORTANT: User previously decided to use JWT tokens..."
}

// Block and provide feedback (PostToolUse)
{
  "decision": "block",
  "reason": "Security issue detected: SQL injection risk in query builder. Consider using parameterized queries."
}

// Continue with context (PostToolUse)
{
  "hookSpecificOutput": {
    "hookEventName": "PostToolUse",
    "additionalContext": "Note: This file was last modified 3 days ago. Related tests in tests/auth_test.rs may need updating."
  }
}

// Force Claude to continue (Stop hook)
{
  "decision": "block",
  "reason": "Goal 'Implement auth system' is only 25% complete. Remaining: JWT handling, login endpoints, tests."
}
```

## Hook Handlers

### SessionStart Handler

```rust
// mira hook session-start
pub async fn handle_session_start(input: SessionStartInput) -> HookOutput {
    // 1. Initialize/resume Mira session
    let session = mira.session_start(&input.cwd, input.session_id).await?;

    // 2. Load project context
    let recap = mira.get_session_recap().await?;

    // 3. Check active goals
    let goals = mira.list_goals(GoalFilter::InProgress).await?;

    // 4. Build context injection
    let mut context = Vec::new();

    if let Some(recap) = recap {
        context.push(format!("Session recap:\n{}", recap));
    }

    if !goals.is_empty() {
        context.push(format!("Active goals:\n{}", format_goals(&goals)));
    }

    HookOutput {
        additional_context: Some(context.join("\n\n")),
        ..Default::default()
    }
}
```

### UserPromptSubmit Handler

```rust
// mira hook user-prompt
pub async fn handle_user_prompt(input: UserPromptInput) -> HookOutput {
    let prompt = &input.prompt;

    // 1. Semantic memory recall
    let memories = mira.recall(prompt, RecallOptions {
        limit: 5,
        threshold: 0.7,
    }).await?;

    // 2. Check for relevant code context
    let code_context = mira.search_code(prompt, 3).await?;

    // 3. Check active goals for relevance
    let goal_context = mira.check_goal_relevance(prompt).await?;

    // 4. Build context injection
    let mut context_parts = Vec::new();

    if !memories.is_empty() {
        context_parts.push(format_memories(&memories));
    }

    if !code_context.is_empty() {
        context_parts.push(format_code_context(&code_context));
    }

    if let Some(goal) = goal_context {
        context_parts.push(format!("Relevant goal: {} ({}%)", goal.title, goal.progress));
    }

    HookOutput {
        additional_context: if context_parts.is_empty() {
            None
        } else {
            Some(context_parts.join("\n\n"))
        },
        ..Default::default()
    }
}
```

### PostToolUse Handler (Write/Edit)

```rust
// mira hook post-tool
pub async fn handle_post_tool(input: PostToolInput) -> HookOutput {
    let file_path = input.tool_input.get("file_path")
        .and_then(|v| v.as_str());

    let Some(file_path) = file_path else {
        return HookOutput::default();
    };

    // 1. Queue file for re-indexing
    mira.queue_file_index(file_path).await?;

    // 2. Quick security scan (async, don't block)
    let security_issues = mira.quick_security_scan(file_path).await?;

    // 3. Check if tests might need updating
    let test_hint = mira.check_related_tests(file_path).await?;

    // Build response
    let mut context_parts = Vec::new();

    if !security_issues.is_empty() {
        // Don't block, but inform Claude
        context_parts.push(format!(
            "⚠️ Potential issues detected:\n{}",
            security_issues.iter().map(|i| format!("- {}", i)).collect::<Vec<_>>().join("\n")
        ));
    }

    if let Some(hint) = test_hint {
        context_parts.push(hint);
    }

    HookOutput {
        additional_context: if context_parts.is_empty() {
            None
        } else {
            Some(context_parts.join("\n\n"))
        },
        ..Default::default()
    }
}
```

### Stop Handler

```rust
// mira hook stop
pub async fn handle_stop(input: StopInput) -> HookOutput {
    // Don't create infinite loops
    if input.stop_hook_active {
        return HookOutput::default();
    }

    // Check if there are incomplete goals that were being worked on
    let active_goal = mira.get_current_working_goal().await?;

    if let Some(goal) = active_goal {
        if goal.progress < 100 && goal.has_pending_milestones() {
            // Optionally prompt Claude to continue
            // Only do this if explicitly configured
            if mira.config.auto_continue_goals {
                return HookOutput {
                    decision: Some("block"),
                    reason: Some(format!(
                        "Goal '{}' is {}% complete. Next milestone: {}",
                        goal.title,
                        goal.progress,
                        goal.next_milestone().map(|m| m.title).unwrap_or("unknown")
                    )),
                    ..Default::default()
                };
            }
        }
    }

    // Save session state
    mira.save_session_state().await?;

    HookOutput::default()
}
```

### PreCompact Handler

```rust
// mira hook pre-compact
pub async fn handle_pre_compact(input: PreCompactInput) -> HookOutput {
    // Extract important context from transcript before compaction
    if let Some(transcript_path) = input.transcript_path {
        let transcript = fs::read_to_string(&transcript_path)?;

        // Extract decisions, TODOs, errors
        let extracted = mira.extract_transcript_context(&transcript).await?;

        // Store as memories with low confidence (auto-extracted)
        for item in extracted {
            mira.remember(RememberParams {
                content: item.content,
                category: Some(item.category),
                confidence: 0.4,
                fact_type: "extracted",
                ..Default::default()
            }).await?;
        }
    }

    HookOutput::default()
}
```

## Skills

### /mira:search

```markdown
---
name: search
description: Search codebase semantically. Use when user asks to find code by concept or functionality.
---

Search the codebase using Mira's semantic search.

Query: $ARGUMENTS

Use the mcp__mira__search_code tool with the query above.
Present results clearly with file paths and relevant snippets.
```

### /mira:recap

```markdown
---
name: recap
description: Get session recap including recent context, preferences, and goals.
---

Get the current session recap from Mira.

Use the mcp__mira__get_session_recap tool.
Present the recap in a clear, organized format.
```

### /mira:goals

```markdown
---
name: goals
description: List and manage cross-session goals and milestones.
---

Manage goals with Mira.

Command: $ARGUMENTS

If no arguments, list all goals using mcp__mira__goal with action="list".
Otherwise, parse the command:
- "add <title>" → create goal
- "progress <id>" → show goal details
- "complete <milestone_id>" → mark milestone done
```

## Internal Event Processing

The hooks call into Mira's core through a unified interface:

```rust
pub struct MiraHookHandler {
    db: Arc<DatabasePool>,
    embeddings: Arc<EmbeddingClient>,
    config: MiraConfig,
}

impl MiraHookHandler {
    /// Process any hook event
    pub async fn handle(&self, event: HookEvent) -> Result<HookOutput> {
        match event {
            HookEvent::SessionStart(input) => self.handle_session_start(input).await,
            HookEvent::UserPrompt(input) => self.handle_user_prompt(input).await,
            HookEvent::PostTool(input) => self.handle_post_tool(input).await,
            HookEvent::PreCompact(input) => self.handle_pre_compact(input).await,
            HookEvent::Stop(input) => self.handle_stop(input).await,
        }
    }
}
```

### CLI Integration

```bash
# Hook subcommand routes to appropriate handler
mira hook <event-name>

# Reads JSON from stdin, writes JSON to stdout
echo '{"session_id":"abc","prompt":"find auth code"}' | mira hook user-prompt
# Output: {"hookSpecificOutput":{"additionalContext":"..."}}
```

## Configuration

### Plugin Config (in plugin.json or separate config)

```json
{
  "mira": {
    "features": {
      "auto_recall": true,
      "code_analysis": true,
      "goal_tracking": true,
      "auto_continue_goals": false
    },
    "recall": {
      "threshold": 0.7,
      "max_results": 5,
      "include_code": true
    },
    "code_analysis": {
      "security_scan": true,
      "test_hints": true
    }
  }
}
```

### User Overrides (~/.mira/config.toml)

```toml
[hooks]
enabled = true
timeout_seconds = 10

[hooks.user_prompt]
enabled = true
min_prompt_length = 10  # Don't process very short prompts

[hooks.post_tool]
enabled = true
security_scan = true

[hooks.stop]
auto_continue_goals = false  # Don't auto-continue, just inform
```

## Implementation Phases

### Phase 1: Plugin Scaffolding
- [ ] Create plugin directory structure
- [ ] Write plugin.json manifest
- [ ] Bundle existing MCP server in .mcp.json
- [ ] Test plugin installation with `claude --plugin-dir`

### Phase 2: Hook Infrastructure
- [ ] Add `mira hook <event>` CLI subcommand
- [ ] Implement hook input/output JSON parsing
- [ ] Create HookHandler trait and implementations
- [ ] Write hooks/hooks.json configuration

### Phase 3: Hook Handlers
- [ ] SessionStart: session init + recap injection
- [ ] UserPromptSubmit: memory recall + code context
- [ ] PostToolUse: file indexing + security hints
- [ ] PreCompact: transcript extraction
- [ ] Stop: goal status + session save

### Phase 4: Skills
- [ ] /mira:search - semantic code search
- [ ] /mira:recap - session recap
- [ ] /mira:goals - goal management
- [ ] /mira:remember - quick memory storage

### Phase 5: Polish
- [ ] Configuration system
- [ ] Error handling and graceful degradation
- [ ] Performance optimization (async, caching)
- [ ] Documentation and README

## Comparison: Plugin vs Previous WebSocket Design

| Aspect | WebSocket Design | Plugin Design |
|--------|------------------|---------------|
| Transport | Custom WebSocket server | Claude Code's hook system |
| Complexity | High (connection mgmt) | Low (stdin/stdout) |
| Reliability | Connection can drop | Each hook is independent |
| Latency | Lower (persistent conn) | Slightly higher (process spawn) |
| Push capability | Full push anytime | Only during hook events |
| Installation | Manual setup | One-click plugin install |
| Maintenance | Two systems to maintain | Single unified system |

**Key tradeoff**: We lose arbitrary push capability (can't send alerts outside of hook events), but gain simplicity and reliability. In practice, the hook events cover all the important moments:
- SessionStart: inject context at start
- UserPromptSubmit: inject context per message
- PostToolUse: react to file changes
- Stop: influence when Claude stops

## Open Questions

1. **Hook Timeout**: Default 60s, but we set lower. What's the right balance between thoroughness and responsiveness?

2. **Context Budget**: How much context to inject per hook? Need to balance helpfulness vs token usage.

3. **Caching**: Should we cache context between hooks to reduce latency? Risk of stale data.

4. **Parallel Hooks**: Claude Code runs matching hooks in parallel. How to handle potential conflicts?

## References

- [Claude Code Plugins](https://code.claude.com/docs/en/plugins)
- [Claude Code Hooks Reference](https://code.claude.com/docs/en/hooks)
- [Plugins Reference](https://code.claude.com/docs/en/plugins-reference)
- Current Mira hooks: `crates/mira-server/src/hooks/`
