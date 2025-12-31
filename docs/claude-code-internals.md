# Claude Code Internal Architecture Map

**Version:** 2.0.76
**Build Date:** 2025-12-22T23:56:23Z
**Binary Location:** `/home/peter/.local/share/claude/versions/2.0.76`
**Type:** Native ELF 64-bit executable (bundled Node.js/JS)
**Last Updated:** 2025-12-28

---

## Table of Contents

1. [System Prompt Templates](#1-system-prompt-templates)
2. [Context Window Management](#2-context-window-management)
3. [Environment Variables](#3-key-environment-variables)
4. [Tool System Architecture](#4-tool-system-architecture)
5. [MCP Integration](#5-mcp-integration-points)
6. [Hooks System (Complete Reference)](#6-hooks-system)
7. [Skills System](#7-skills-system)
8. [Subagents & Custom Agents](#8-subagents--custom-agents)
9. [Plugin System](#9-plugin-system)
10. [CLI Reference & Streaming](#10-cli-reference--streaming)
11. [Settings System](#11-settings-system)
12. [Permission System](#12-permission-system)
13. [IDE Integration](#13-ide-integration)
14. [Integration Points for Orchestrator](#14-integration-points-for-orchestrator)
15. [Beta Features & Recent Additions](#15-beta-features--recent-additions)
16. [Undocumented/Internal Features](#16-undocumentedinternal-features)
17. [Known Issues & Limitations](#17-known-issues--limitations)
18. [Official Plugins Reference](#18-official-plugins-reference)
19. [Complete Environment Variable Reference](#19-complete-environment-variable-reference)
20. [Useful Patterns for Orchestration](#20-useful-patterns-for-orchestration)

---

## 1. System Prompt Templates

### Core Identity Prompts
```javascript
Re$ = "You are Claude Code, Anthropic's official CLI for Claude."
YXI = "You are Claude Code, Anthropic's official CLI for Claude, running within the Claude Agent SDK."
wXI = "You are a Claude agent, built on Anthropic's Claude Agent SDK."
```

### Mode Selection Logic
```javascript
function u0$(H) {
  if (k0() === "vertex") return Re$;
  if (H?.isNonInteractive) {
    if (H.hasAppendSystemPrompt) return YXI;
    return wXI;
  }
  return Re$;
}
```

---

## 2. Context Window Management

### Token Limits (Default)
| Parameter | Default Value | Environment Variable |
|-----------|---------------|---------------------|
| Max Input Tokens | **180,000** | `API_MAX_INPUT_TOKENS` |
| Target Input Tokens | **40,000** | `API_TARGET_INPUT_TOKENS` |

### Automatic Context Clearing Strategy
```javascript
// From binary analysis:
VXI = 180000  // Max trigger threshold
QXI = 40000   // Target after clearing

// Clear at least (max - target) = 140,000 tokens when triggered
```

### Clear Tool Uses Configuration
```javascript
if (process.env.USE_API_CLEAR_TOOL_RESULTS) {
  // Clears tool result content when approaching limits
  E = {
    type: "clear_tool_uses_20250919",
    trigger: { type: "input_tokens", value: B },
    clear_at_least: { type: "input_tokens", value: B - f },
    clear_tool_inputs: Eu0  // Excluded tools list
  }
}

if (process.env.USE_API_CLEAR_TOOL_USES) {
  // Clears entire tool uses
  exclude_tools: Mu0  // Protected tools
}
```

### Thinking Preservation
```javascript
// When thinking mode is enabled:
if (A && $) {
  B = { type: "clear_thinking_20251015", keep: "all" }
}
```

---

## 3. Key Environment Variables

### API/Model Configuration
| Variable | Purpose |
|----------|---------|
| `ANTHROPIC_API_KEY` | API authentication |
| `ANTHROPIC_MODEL` | Override default model |
| `ANTHROPIC_DEFAULT_SONNET_MODEL` | Sonnet model ID |
| `ANTHROPIC_DEFAULT_OPUS_MODEL` | Opus model ID |
| `ANTHROPIC_DEFAULT_HAIKU_MODEL` | Haiku model ID |
| `ANTHROPIC_SMALL_FAST_MODEL` | Fast model for lightweight tasks |

### Context Management
| Variable | Purpose |
|----------|---------|
| `API_MAX_INPUT_TOKENS` | Override max context (default: 180000) |
| `API_TARGET_INPUT_TOKENS` | Override target after clearing (default: 40000) |
| `USE_API_CLEAR_TOOL_RESULTS` | Enable tool result clearing |
| `USE_API_CLEAR_TOOL_USES` | Enable full tool use clearing |
| `USE_API_CONTEXT_MANAGEMENT` | Enable API-side context management |
| `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` | Autocompact threshold percentage |

### Bash/Tool Configuration
| Variable | Purpose |
|----------|---------|
| `BASH_DEFAULT_TIMEOUT_MS` | Default bash command timeout |
| `BASH_MAX_TIMEOUT_MS` | Maximum bash timeout |
| `BASH_MAX_OUTPUT_LENGTH` | Truncation limit for bash output |
| `CLAUDE_CODE_MAX_TOOL_USE_CONCURRENCY` | Parallel tool execution limit |

### Telemetry/Debugging
| Variable | Purpose |
|----------|---------|
| `ENABLE_ENHANCED_TELEMETRY_BETA` | Detailed telemetry |
| `OTEL_LOG_TOOL_CONTENT` | Log tool inputs/outputs |
| `OTEL_LOG_USER_PROMPTS` | Log user prompts |
| `CLAUDE_CODE_DEBUG_LOGS_DIR` | Debug log directory |
| `CLAUDE_CODE_DIAGNOSTICS_FILE` | Diagnostics output |

### Session/Agent Configuration
| Variable | Purpose |
|----------|---------|
| `CLAUDE_CODE_SESSION_ID` | Current session identifier |
| `CLAUDE_CODE_PARENT_SESSION_ID` | Parent session for subagents |
| `CLAUDE_CODE_AGENT_ID` | Agent identifier |
| `CLAUDE_CODE_AGENT_NAME` | Agent display name |
| `CLAUDE_CODE_AGENT_TYPE` | Agent type classification |
| `CLAUDE_CODE_SUBAGENT_MODEL` | Model for spawned subagents |

### Feature Flags
| Variable | Purpose |
|----------|---------|
| `DISABLE_PROMPT_CACHING` | Disable all caching |
| `DISABLE_PROMPT_CACHING_OPUS` | Disable for Opus only |
| `DISABLE_INTERLEAVED_THINKING` | Disable thinking mode |
| `DISABLE_MICROCOMPACT` | Disable micro-compaction |
| `DISABLE_COMPACT` | Disable compaction |
| `ENABLE_TOOL_SEARCH` | Enable tool search feature |
| `ENABLE_LSP_TOOL` | Enable LSP integration |

---

## 4. Tool System Architecture

### Tool Exclusion Lists
```javascript
// Tools excluded from clearing (critical tools):
Eu0 = [_0, WF, ZJ, k8, f4, q_]
// Mapped to: Read, Edit, Write, Glob, Grep, WebSearch

// Tools protected during context management:
Mu0 = [u8, G4, z_]
// Mapped to: Bash, Task, TodoWrite
```

### Telemetry Tracking for Tools
```javascript
function q8D(H, $) {
  // Tracks tool invocations with:
  // - tool_name
  // - start_time
  // - duration
  // - success/failure
  // - content (if OTEL_LOG_TOOL_CONTENT)
}
```

---

## 5. MCP Integration Points

| Variable | Purpose |
|----------|---------|
| `MCP_TIMEOUT` | MCP server connection timeout |
| `MCP_TOOL_TIMEOUT` | Individual tool execution timeout |
| `MCP_OAUTH_CALLBACK_PORT` | OAuth callback port |
| `MCP_SERVER_CONNECTION_BATCH_SIZE` | Batch connection size |
| `MAX_MCP_OUTPUT_TOKENS` | Max tokens for MCP tool output |
| `ENABLE_MCP_LARGE_OUTPUT_FILES` | Allow large file outputs |
| `USE_MCP_CLI_DIR` | MCP CLI directory |

---

## 6. Hooks System

User-defined shell commands that execute at various points in Claude Code's lifecycle.

### Hook Events

| Event | Trigger | Matcher | Use Case |
|-------|---------|---------|----------|
| **PreToolUse** | Before tool execution | Yes | Approve/deny/modify tool calls |
| **PermissionRequest** | Permission dialog shown | Yes | Auto-approve/deny permissions |
| **PostToolUse** | After tool completes | Yes | Validate results, provide feedback |
| **Notification** | Notification sent | Yes | Handle different notification types |
| **UserPromptSubmit** | User submits prompt | No | Validate/block prompts, add context |
| **Stop** | Main agent finishes | No | Prevent premature stopping |
| **SubagentStop** | Subagent finishes | No | Control subagent completion |
| **PreCompact** | Before compaction | Yes (`manual`, `auto`) | Run before context compaction |
| **SessionStart** | Session begins/resumes | Yes (`startup`, `resume`, `clear`, `compact`) | Load development context |
| **SessionEnd** | Session ends | No | Cleanup tasks |

### Configuration Structure

```json
{
  "hooks": {
    "EventName": [
      {
        "matcher": "ToolPattern",
        "hooks": [
          {
            "type": "command",
            "command": "bash-command",
            "timeout": 60
          }
        ]
      }
    ]
  }
}
```

### Matcher Patterns

- **Exact match**: `Write` matches only Write tool
- **Regex**: `Edit|Write` or `Notebook.*`
- **Match all**: `*`, `""`, or omit `matcher` field
- **MCP tools**: `mcp__<server>__<tool>` pattern (e.g., `mcp__memory__.*`)

### Hook Types

| Type | Description |
|------|-------------|
| `command` | Execute bash script |
| `prompt` | Use LLM to make decisions (Stop/SubagentStop only) |

### Exit Codes

| Code | Behavior |
|------|----------|
| **0** | Success - proceed normally, process JSON in stdout |
| **2** | Block action - prevent tool/action, show stderr to Claude |
| **Other** | Non-blocking error - continue, show stderr in verbose mode |

### Hook Input (stdin JSON)

**Common fields (all hooks):**
```json
{
  "session_id": "string",
  "transcript_path": "/path/to/conversation.jsonl",
  "cwd": "/current/working/directory",
  "permission_mode": "default|plan|acceptEdits|bypassPermissions",
  "hook_event_name": "EventName"
}
```

**PreToolUse:**
```json
{
  "tool_name": "Write",
  "tool_input": { "file_path": "...", "content": "..." },
  "tool_use_id": "toolu_..."
}
```

**PostToolUse:**
```json
{
  "tool_name": "Write",
  "tool_input": { ... },
  "tool_response": { "filePath": "...", "success": true },
  "tool_use_id": "toolu_..."
}
```

**SessionStart:**
```json
{
  "source": "startup|resume|clear|compact"
}
```

### Hook Output (stdout JSON)

**Common fields:**
```json
{
  "continue": true,
  "stopReason": "Message when continue=false",
  "suppressOutput": false,
  "systemMessage": "Optional warning"
}
```

**PreToolUse decision control:**
```json
{
  "hookSpecificOutput": {
    "hookEventName": "PreToolUse",
    "permissionDecision": "allow|deny|ask",
    "permissionDecisionReason": "explanation",
    "updatedInput": { "field": "modified_value" }
  }
}
```

**PermissionRequest decision:**
```json
{
  "hookSpecificOutput": {
    "hookEventName": "PermissionRequest",
    "decision": {
      "behavior": "allow|deny",
      "updatedInput": { ... },
      "message": "Why denied"
    }
  }
}
```

### Environment Variables for Hooks

| Variable | Description |
|----------|-------------|
| `CLAUDE_PROJECT_DIR` | Absolute path to project root |
| `CLAUDE_CODE_REMOTE` | `"true"` for web, empty for CLI |
| `CLAUDE_ENV_FILE` | Path to persist env vars (SessionStart only) |
| `CLAUDE_PLUGIN_ROOT` | Plugin root directory (plugin hooks) |

### Example: Auto-format on file change
```json
{
  "hooks": {
    "PostToolUse": [{
      "matcher": "Edit|Write",
      "hooks": [{
        "type": "command",
        "command": "prettier --write \"$CLAUDE_FILE_PATHS\""
      }]
    }]
  }
}
```

### Example: Permission auto-approval
```json
{
  "hooks": {
    "PermissionRequest": [{
      "matcher": "Bash",
      "hooks": [{
        "type": "command",
        "command": "python3 check_bash_permission.py"
      }]
    }]
  }
}
```

---

## 7. Skills System

Model-invoked specialized knowledge that Claude loads automatically based on context.

### SKILL.md Format

```markdown
---
name: your-skill-name
description: Brief description (max 1024 chars) - Claude uses this to decide when to use
allowed-tools: Read, Grep, Glob
model: claude-sonnet-4-20250514
---

# Skill Instructions

Step-by-step guidance for Claude...
```

### YAML Frontmatter Fields

| Field | Required | Description |
|-------|----------|-------------|
| `name` | Yes | Lowercase letters, numbers, hyphens (max 64 chars) |
| `description` | Yes | What/when to use (max 1024 chars) |
| `allowed-tools` | No | Tools Claude can use without asking |
| `model` | No | Model override for this skill |

### Skill Locations

| Location | Path | Scope |
|----------|------|-------|
| Enterprise | Managed settings | All org users |
| Personal | `~/.claude/skills/` | You, all projects |
| Project | `.claude/skills/` | Repository collaborators |
| Plugin | Plugin `skills/` dir | Plugin users |

**Priority:** Enterprise > Personal > Project > Plugin

### Multi-File Skills

```
my-skill/
├── SKILL.md              # Required - overview
├── reference.md          # Detailed docs (linked from SKILL.md)
├── examples.md           # Usage examples
└── scripts/
    └── helper.py         # Utility scripts
```

**Best practice:** Keep `SKILL.md` under 500 lines, link to supporting files.

---

## 8. Subagents & Custom Agents

### Built-in Subagents

| Agent | Model | Tools | Use Case |
|-------|-------|-------|----------|
| **general-purpose** | Sonnet | All | Complex multi-step tasks |
| **Plan** | Sonnet | Read, Glob, Grep, Bash | Codebase research for planning |
| **Explore** | Haiku | Glob, Grep, Read (read-only) | Fast codebase exploration |

### Custom Agent Location

| Type | Location |
|------|----------|
| Project | `.claude/agents/` |
| User | `~/.claude/agents/` |

### AGENT.md Format

```markdown
---
name: your-agent-name
description: When this agent should be invoked
tools: Read, Write, Edit, Bash, Glob, Grep
model: sonnet
permissionMode: default
skills: skill1, skill2
---

You are a [role description]...

When invoked:
1. Step one
2. Step two
...
```

### Agent Configuration Fields

| Field | Required | Options |
|-------|----------|---------|
| `name` | Yes | Lowercase letters, hyphens |
| `description` | Yes | Include "PROACTIVELY" for auto-delegation |
| `tools` | No | Comma-separated (inherits all if omitted) |
| `model` | No | `sonnet`, `opus`, `haiku`, `'inherit'` |
| `permissionMode` | No | `default`, `acceptEdits`, `bypassPermissions`, `plan`, `ignore` |
| `skills` | No | Comma-separated skill names |

### CLI-Based Agent Definition

```bash
claude --agents '{
  "code-reviewer": {
    "description": "Expert code reviewer",
    "prompt": "You are a senior code reviewer...",
    "tools": ["Read", "Grep", "Glob", "Bash"],
    "model": "sonnet"
  }
}'
```

### Resumable Agents

Agents can be resumed with full context using their `agentId`:
```
> Resume agent {agentId} and continue with...
```

---

## 9. Plugin System

### Plugin Structure

```
my-plugin/
├── .claude-plugin/
│   └── plugin.json          # Required manifest
├── commands/                # Slash commands
│   └── hello.md
├── agents/                  # Custom agents
├── skills/                  # Agent Skills
│   └── code-review/
│       └── SKILL.md
├── hooks/                   # Event handlers
│   └── hooks.json
├── .mcp.json               # MCP server configs
├── .lsp.json               # LSP server configs
└── README.md
```

**Important:** Only `plugin.json` goes in `.claude-plugin/`. All other dirs at plugin root.

### plugin.json Schema

```json
{
  "name": "my-first-plugin",
  "description": "Plugin description",
  "version": "1.0.0",
  "author": { "name": "Your Name" },
  "homepage": "https://example.com",
  "repository": "https://github.com/user/repo",
  "license": "MIT"
}
```

### Slash Commands

**Location:** `commands/*.md`
**Naming:** `filename.md` → `/plugin-name:filename`

```markdown
---
description: Greet the user
---

# Hello Command

Greet the user named "$ARGUMENTS" warmly.
```

**Argument placeholders:** `$ARGUMENTS`, `$1`, `$2`

### Plugin Hooks

**Location:** `hooks/hooks.json`

```json
{
  "hooks": {
    "PostToolUse": [{
      "matcher": "Write|Edit",
      "hooks": [{
        "type": "command",
        "command": "${CLAUDE_PLUGIN_ROOT}/scripts/format.sh"
      }]
    }]
  }
}
```

### Development & Testing

```bash
# Test single plugin
claude --plugin-dir ./my-plugin

# Multiple plugins
claude --plugin-dir ./plugin-one --plugin-dir ./plugin-two
```

### Plugin Marketplaces

```json
{
  "extraKnownMarketplaces": {
    "acme-tools": {
      "source": {
        "source": "github",
        "repo": "acme-corp/claude-plugins",
        "ref": "main",
        "path": "marketplace"
      }
    }
  }
}
```

**Source types:** `github`, `git`, `url`, `npm`, `file`, `directory`

---

## 10. CLI Reference & Streaming

### Output Format Options

| Flag | Description |
|------|-------------|
| `--output-format text` | Plain text (default) |
| `--output-format json` | Structured JSON |
| `--output-format stream-json` | Streaming JSON (JSONL) |

### Stream-JSON Format

```bash
claude -p "query" --output-format stream-json | tee output.jsonl
```

**Output structure:**
```json
{ "type": "text", "text": "I'll write..." }
{ "type": "tool_use", "id": "toolu_...", "name": "Write", "input": {...} }
{ "tool_use_id": "toolu_...", "type": "tool_result", "content": "..." }
```

### Non-Interactive Mode Flags

| Flag | Purpose |
|------|---------|
| `-p "query"` / `--print` | Execute and exit |
| `--max-turns N` | Limit agentic turns |
| `--include-partial-messages` | Include streaming partials |
| `--input-format stream-json` | Accept streaming JSON input |
| `--fallback-model` | Enable overload fallback |

### Permission Control

```bash
# Skip all permissions (dangerous)
claude --dangerously-skip-permissions -p "query"

# Set permission mode
claude --permission-mode plan -p "query"

# Allow specific tools
claude --allowedTools "Bash(git log:*)" "Bash(git diff:*)" "Read"

# Disallow tools
claude --disallowedTools "WebFetch" "Edit"
```

### Session Management

```bash
# Continue previous session
claude -c -p "query"

# Resume specific session
claude -r "session-id" -p "query"

# Fork session
claude --resume abc123 --fork-session
```

### Model & System Prompt

```bash
# Set model
claude --model claude-sonnet-4-5-20250929
claude --model sonnet  # alias

# Custom system prompt
claude --system-prompt "You are a Python expert"
claude --append-system-prompt "Always use TypeScript"  # Preserves defaults
claude --system-prompt-file ./custom-prompt.txt
```

### Structured Output

```bash
claude -p --json-schema '{"type":"object","properties":{...}}' "query"
```

### Debugging

```bash
claude --debug "api,hooks"        # Enable specific categories
claude --debug "!statsig,!file"   # Exclude categories
claude --verbose -p "query"       # Verbose output
```

---

## 11. Settings System

### Configuration Files

| File | Scope | Shared |
|------|-------|--------|
| `~/.claude/settings.json` | User | No |
| `.claude/settings.json` | Project | Yes (git) |
| `.claude/settings.local.json` | Local project | No (gitignored) |
| `managed-settings.json` | Enterprise | Yes |

**Precedence:** Enterprise > CLI args > Local > Shared > User

**Enterprise paths:**
- macOS: `/Library/Application Support/ClaudeCode/`
- Linux: `/etc/claude-code/`
- Windows: `C:\Program Files\ClaudeCode\`

### Core Settings

| Key | Type | Description |
|-----|------|-------------|
| `model` | string | Default model |
| `alwaysThinkingEnabled` | boolean | Enable extended thinking |
| `outputStyle` | string | Output style for prompts |
| `cleanupPeriodDays` | number | Delete inactive sessions after N days |
| `forceLoginMethod` | string | `"claudeai"` or `"console"` |

### Attribution Settings

```json
{
  "attribution": {
    "commit": "Generated with Claude Code\n\nCo-Authored-By: Claude <noreply@anthropic.com>",
    "pr": "Generated with Claude Code"
  }
}
```

### MCP Server Settings

| Key | Description |
|-----|-------------|
| `enableAllProjectMcpServers` | Auto-approve all project MCP servers |
| `enabledMcpjsonServers` | Approved MCP servers list |
| `disabledMcpjsonServers` | Rejected MCP servers list |
| `allowedMcpServers` | Enterprise allowlist |
| `deniedMcpServers` | Enterprise denylist |

### Hook Settings

| Key | Description |
|-----|-------------|
| `hooks` | Hook configuration object |
| `disableAllHooks` | Disable all hooks |
| `allowManagedHooksOnly` | Enterprise: only managed hooks |

### Environment Settings

```json
{
  "env": {
    "CLAUDE_CODE_MAX_OUTPUT_TOKENS": "16384",
    "BASH_DEFAULT_TIMEOUT_MS": "30000"
  }
}
```

### Sandbox Settings

```json
{
  "sandbox": {
    "enabled": true,
    "autoAllowBashIfSandboxed": true,
    "excludedCommands": ["git", "docker"],
    "allowUnsandboxedCommands": true,
    "network": {
      "allowUnixSockets": ["~/.ssh/agent-socket"],
      "allowLocalBinding": false,
      "httpProxyPort": 8080,
      "socksProxyPort": 8081
    },
    "enableWeakerNestedSandbox": false
  }
}
```

| Key | Type | Description |
|-----|------|-------------|
| `enabled` | boolean | Enable bash sandboxing (macOS/Linux only) |
| `autoAllowBashIfSandboxed` | boolean | Auto-approve bash in sandbox |
| `excludedCommands` | array | Commands to run outside sandbox |
| `allowUnsandboxedCommands` | boolean | Allow `dangerouslyDisableSandbox` escape |
| `network.allowUnixSockets` | array | Unix sockets accessible in sandbox |
| `network.allowLocalBinding` | boolean | Allow localhost binding (macOS) |
| `enableWeakerNestedSandbox` | boolean | Weaker sandbox for Docker (Linux) |

### Plugin Settings

```json
{
  "enabledPlugins": {
    "formatter@team-tools": true,
    "analyzer@security-plugins": false
  },
  "extraKnownMarketplaces": {
    "team-tools": {
      "source": { "source": "github", "repo": "org/repo", "ref": "main" }
    }
  },
  "strictKnownMarketplaces": []  // Enterprise: empty = complete lockdown
}
```

---

## 12. Permission System

### Permission Rules Format

```json
{
  "permissions": {
    "allow": ["Bash(npm run:*)", "Read(src/**)"],
    "ask": ["Bash(git push:*)"],
    "deny": ["WebFetch", "Bash(curl:*)", "Read(.env)"],
    "additionalDirectories": ["../docs/"],
    "defaultMode": "acceptEdits",
    "disableBypassPermissionsMode": "disable"
  }
}
```

### Rule Format

```
Tool(pattern)
```

**Examples:**
- `Bash(npm run:*)` - All npm run commands
- `Read(.env*)` - All .env files
- `WebFetch` - All web fetch operations

### Permission Modes

| Mode | Description |
|------|-------------|
| `default` | Ask for permission |
| `acceptEdits` | Auto-approve file edits |
| `bypassPermissions` | Skip all permissions |
| `plan` | Read-only exploration |

### Tools and Permission Requirements

| Tool | Permission |
|------|------------|
| Bash | Yes |
| Read | No |
| Write | Yes |
| Edit | Yes |
| Glob | No |
| Grep | No |
| WebFetch | Yes |
| WebSearch | Yes |
| Task | No |
| TodoWrite | No |

---

## 13. IDE Integration

### VS Code Extension

Claude Code VS Code extension uses WebSocket for communication.

**Port range:** 42000-44000

**Lock files:** `~/.claude/ide-*.lock`

**Connection:** The CLI connects to the extension's WebSocket server.

### WebSocket Authentication

Starting v1.0.24, WebSocket connections require authentication:
- Auth token stored in lock file
- CLI provides token during WebSocket connection
- Prevents CVE-2025-52882 (unauthenticated command execution)

### IDE Commands

```bash
/ide           # Connect to IDE
/config        # Set diff tool to 'auto' for IDE detection
```

### Environment Variables

| Variable | Purpose |
|----------|---------|
| `CLAUDE_CODE_AUTO_CONNECT_IDE` | Auto-connect to IDE |
| `CLAUDE_CODE_IDE_HOST_OVERRIDE` | Override IDE host |
| `CLAUDE_CODE_IDE_SKIP_AUTO_INSTALL` | Skip extension auto-install |

---

## 14. Integration Points for Orchestrator

### Session Management
```javascript
// Session polling endpoint pattern
/v1/sessions/${H}/events

// Session creation endpoint
${wB().BASE_API_URL}/v1/sessions
```

### Telemetry Spans
```javascript
// Interaction span: claude_code.interaction
// LLM request span: claude_code.llm_request
// Tool span: claude_code.tool
// Hook span: claude_code.hook
```

### Environment Injection Points
```javascript
// Pass to spawned processes:
CLAUDE_CODE_PARENT_SESSION_ID
CLAUDE_CODE_AGENT_ID
CLAUDE_CODE_SESSION_ACCESS_TOKEN
```

### Stream Chaining (Multi-Agent Pipelines)

Connect multiple Claude Code processes using real-time JSON streams:

```bash
# Pipe output from one Claude to another
claude -p "analyze code" --output-format stream-json | \
  claude -p "refactor based on analysis" --input-format stream-json
```

### Note on CLAUDE_CODE_SSE_PORT
Despite appearing in some documentation, `CLAUDE_CODE_SSE_PORT` does **NOT** create a local
SSE server. Testing confirms:
- Setting this env var does not open any listening ports
- The string is not present in the compiled binary (v2.0.76)
- Claude Code only makes outbound HTTPS connections to Anthropic's API

**Correct approach for real-time events:** Use `--output-format stream-json` and parse stdout.
This provides JSON-formatted streaming events for all Claude Code activity.

### Orchestration via Hooks

Use hooks to integrate with external orchestration systems:

```json
{
  "hooks": {
    "SessionStart": [{
      "hooks": [{
        "type": "command",
        "command": "curl -X POST http://localhost:8080/session/start"
      }]
    }],
    "PostToolUse": [{
      "matcher": "*",
      "hooks": [{
        "type": "command",
        "command": "python3 notify_orchestrator.py"
      }]
    }]
  }
}
```

---

## 15. Beta Features & Recent Additions

### Claude in Chrome (v2.0.72+)

Browser automation via Chrome extension. Enables direct browser control from Claude Code.

```bash
# Requires Chrome extension installation
# Extension provides MCP tools for browser interaction
```

**Capabilities:**
- Navigate to URLs
- Read page content (accessibility tree)
- Click elements, fill forms
- Take screenshots
- Execute JavaScript in page context
- GIF recording of browser sessions

### LSP Tool (v2.0.74+)

Language Server Protocol integration for enhanced code intelligence.

```bash
# Enable via environment variable
ENABLE_LSP_TOOL=true claude
```

**Features:**
- Go to definition
- Find references
- Symbol search
- Diagnostics integration

**Known limitation:** Cannot detect language servers from plugins (#15641)

### Named Sessions (v2.0.64+)

Named session management for easier context switching.

```bash
# Rename current session
/rename my-feature-work

# Resume by name
claude --resume my-feature-work
```

### Background Agents (v2.0.60+)

Subagents that run in the background while you continue working.

```bash
# Agents can be spawned with run_in_background: true
# Use TaskOutput tool to retrieve results later
```

### Thinking Mode (v2.0.67+)

Extended thinking enabled by default for Opus 4.5.

```bash
# Toggle with keyboard shortcut
Alt+T  # Linux/Windows
Option+T  # macOS

# Or configure via /config
```

### Progress Bars (OSC 9;4)

Terminal progress bar support for supported terminals.

```bash
# Toggle via settings
terminal-progress-bar: true/false
```

---

## 16. Undocumented/Internal Features

| Feature | Variable/Flag |
|---------|--------------|
| Bubblewrap sandboxing | `CLAUDE_CODE_BUBBLEWRAP` |
| Sandbox indicator | `CLAUDE_CODE_BASH_SANDBOX_SHOW_INDICATOR` |
| Command injection check | `CLAUDE_CODE_DISABLE_COMMAND_INJECTION_CHECK` |
| File checkpointing | `CLAUDE_CODE_DISABLE_FILE_CHECKPOINTING` |
| SDK file checkpointing | `CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING` |
| Remote environment type | `CLAUDE_CODE_REMOTE_ENVIRONMENT_TYPE` |
| Subagent zoom | `ENABLE_SUBAGENT_ZOOM` |
| Code terminal | `FORCE_CODE_TERMINAL` |
| Extra body injection | `CLAUDE_CODE_EXTRA_BODY` |

### Deprecated Features

| Feature | Status | Replacement |
|---------|--------|-------------|
| Output styles | Deprecated | Plugins with SessionStart hooks |
| `#` shortcut | Deprecated | `/settings` or `/config` |

---

## 17. Known Issues & Limitations

Issues discovered from GitHub issue tracker (as of late 2025):

### Permission System Issues

| Issue | Description | Status |
|-------|-------------|--------|
| #15612 | Multi-line commands bypass permission checks | Open |
| #15586 | Chained commands bypass permission checks | Open |
| #15652 | File permission "allow for Edit" not working with paths | Open |
| #15643 | Inconsistent permission triggering in Cursor IDE | Open |

### Hook System Issues

| Issue | Description | Status |
|-------|-------------|--------|
| #15624 | Stop hook executes twice on termination | Open |
| #15629 | User-level stop hook doesn't trigger from home directory | Open |
| #15617 | PostToolUse hooks not firing on Termux | Open |

### Skills & Plugins Issues

| Issue | Description | Status |
|-------|-------------|--------|
| #15635 | Skills accumulate after compaction (no unload mechanism) | Open |
| #15641 | LSP tool can't detect language servers from plugins | Open |

### LSP Tool Race Condition (#15641)

**Status:** Open, no workaround

**Root Cause:** The LSP Manager initializes **before** plugins are loaded. When plugins with `lspServers` configuration load, the LSP Manager has already finished initialization and doesn't pick up the server configs.

**Debug Log Evidence:**
```
20:51:37.311Z [DEBUG] LSP notification handlers registered successfully for all 0 server(s)
20:51:37.313Z [DEBUG] Loading plugin rust-analyzer-lsp from source...
20:51:37.375Z [DEBUG] Checking plugin: skillsPath=none, skillsPaths=0 paths
```

**What's Missing:** The plugin loader extracts skills, commands, hooks, and agents from plugins but **does not** extract `lspServers` and register them with the LSP Manager.

**Configuration Files (Not Working):**
- Project `.lsp.json` - parsed but not connected to LSP Manager
- Plugin manifest `lspServers` field - ignored during plugin load
- `ENABLE_LSP_TOOL=1` - required but not sufficient

**Related Issues:** #13952, #15148, #15521

### Platform-Specific Issues

| Platform | Issue |
|----------|-------|
| Android/Termux | Hardcoded `/tmp/` paths break |
| Windows | UTF-8 multibyte character handling (Korean, Chinese) |
| Docker | Plugin cache versioning issues |

---

## 18. Official Plugins Reference

Claude Code ships with 14+ official plugins demonstrating advanced patterns.

### Ralph Wiggum (Loop Controller)

Self-referential AI loop with completion promises.

```markdown
# Completion Detection
Use exact string matching: <promise>COMPLETE</promise>

# Pattern
- Agent iterates on task
- Checks git history between iterations
- Completes when promise string is output
```

**Key features:**
- Persistent state across iterations
- Git history visible between loops
- Configurable completion detection

### Hookify (Dynamic Rule Engine)

Create hooks dynamically through natural language.

```bash
# Example: Create a formatting hook
/hookify "Format all Python files after editing with black"
```

**Capabilities:**
- Pattern matching for tool names
- Rule persistence across sessions
- Natural language rule definition

### Feature-Dev (7-Phase Development)

Structured feature development with parallel agents.

```
Phase 1: Requirements Analysis
Phase 2: Architecture Design
Phase 3: Implementation Planning
Phase 4: Code Generation
Phase 5: Testing
Phase 6: Documentation
Phase 7: Review & Polish
```

**Key patterns:**
- Multiple specialized agents
- Phase-based orchestration
- Parallel agent execution

### Code-Review (Multi-Agent PR Review)

Comprehensive code review with confidence scoring.

```json
{
  "agents": ["security-reviewer", "performance-reviewer", "style-reviewer"],
  "confidence_threshold": 80,
  "output": "inline_comments"
}
```

**Features:**
- Multiple reviewer agents
- Confidence-based filtering (threshold: 80)
- GitHub inline comments via MCP
- File path with line range references

### Security-Guidance (Vulnerability Detection)

Pattern detection for common security vulnerabilities.

**Detects 9 vulnerability categories:**
1. SQL Injection
2. XSS (Cross-Site Scripting)
3. Command Injection
4. Path Traversal
5. SSRF (Server-Side Request Forgery)
6. Insecure Deserialization
7. Hardcoded Secrets
8. Weak Cryptography
9. Authentication Bypass

```python
# Hook triggers on file writes
# Scans for vulnerability patterns
# Blocks or warns on detection
```

### PR-Review-Toolkit (GitHub Integration)

Full PR workflow automation.

```bash
# Tools available
mcp__github_inline_comment__create_inline_comment
gh pr view, gh pr comment, gh pr diff
```

**Agents:**
- `code-reviewer.md` - Main review logic
- Confidence scoring for issue filtering
- Direct GitHub API integration

### Agent-SDK-Dev (SDK Development)

Tools for building Agent SDK applications.

```bash
/new-sdk-app my-agent
```

**Features:**
- Project scaffolding
- SDK documentation access
- Agent configuration templates

---

## 19. Complete Environment Variable Reference

### Core Configuration
```
ANTHROPIC_API_KEY
ANTHROPIC_AUTH_TOKEN
ANTHROPIC_BASE_URL
ANTHROPIC_BETAS
ANTHROPIC_CUSTOM_HEADERS
ANTHROPIC_MODEL
ANTHROPIC_DEFAULT_HAIKU_MODEL
ANTHROPIC_DEFAULT_OPUS_MODEL
ANTHROPIC_DEFAULT_SONNET_MODEL
ANTHROPIC_SMALL_FAST_MODEL
ANTHROPIC_SMALL_FAST_MODEL_AWS_REGION
```

### Provider Configuration
```
CLAUDE_CODE_USE_BEDROCK
CLAUDE_CODE_USE_VERTEX
CLAUDE_CODE_USE_FOUNDRY
ANTHROPIC_BEDROCK_BASE_URL
ANTHROPIC_FOUNDRY_API_KEY
ANTHROPIC_FOUNDRY_BASE_URL
ANTHROPIC_FOUNDRY_RESOURCE
ANTHROPIC_VERTEX_PROJECT_ID
BEDROCK_BASE_URL
VERTEX_BASE_URL
VERTEX_REGION_CLAUDE_
VERTEX_REGION_CLAUDE_HAIKU_
```

### Claude Code Specific
```
CLAUDE_CODE_ACTION
CLAUDE_CODE_ADDITIONAL_PROTECTION
CLAUDE_CODE_AGENT_ID
CLAUDE_CODE_AGENT_NAME
CLAUDE_CODE_AGENT_TYPE
CLAUDE_CODE_API_KEY_FILE_DESCRIPTOR
CLAUDE_CODE_API_KEY_HELPER_TTL_MS
CLAUDE_CODE_AUTO_CONNECT_IDE
CLAUDE_CODE_BASH_SANDBOX_SHOW_INDICATOR
CLAUDE_CODE_BUBBLEWRAP
CLAUDE_CODE_CLIENT_CERT
CLAUDE_CODE_CLIENT_KEY
CLAUDE_CODE_CLIENT_KEY_PASSPHRASE
CLAUDE_CODE_CONTAINER_ID
CLAUDE_CODE_DEBUG_LOGS_DIR
CLAUDE_CODE_DIAGNOSTICS_FILE
CLAUDE_CODE_DISABLE_ATTACHMENTS
CLAUDE_CODE_DISABLE_CLAUDE_MDS
CLAUDE_CODE_DISABLE_COMMAND_INJECTION_CHECK
CLAUDE_CODE_DISABLE_EXPERIMENTAL_BETAS
CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY
CLAUDE_CODE_DISABLE_FILE_CHECKPOINTING
CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC
CLAUDE_CODE_DISABLE_TERMINAL_TITLE
CLAUDE_CODE_DONT_INHERIT_ENV
CLAUDE_CODE_EFFORT_LEVEL
CLAUDE_CODE_ENABLE_CFC
CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION
CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING
CLAUDE_CODE_ENABLE_TELEMETRY
CLAUDE_CODE_ENABLE_TOKEN_USAGE_ATTACHMENT
CLAUDE_CODE_ENTRYPOINT
CLAUDE_CODE_EXIT_AFTER_STOP_DELAY
CLAUDE_CODE_EXTRA_BODY
CLAUDE_CODE_FORCE_FULL_LOGO
CLAUDE_CODE_GIT_BASH_PATH
CLAUDE_CODE_IDE_HOST_OVERRIDE
CLAUDE_CODE_IDE_SKIP_AUTO_INSTALL
CLAUDE_CODE_IDE_SKIP_VALID_CHECK
CLAUDE_CODE_MAX_OUTPUT_TOKENS
CLAUDE_CODE_MAX_RETRIES
CLAUDE_CODE_MAX_TOOL_USE_CONCURRENCY
CLAUDE_CODE_OAUTH_TOKEN
CLAUDE_CODE_OAUTH_TOKEN_FILE_DESCRIPTOR
CLAUDE_CODE_OTEL_FLUSH_TIMEOUT_MS
CLAUDE_CODE_OTEL_HEADERS_HELPER_DEBOUNCE_MS
CLAUDE_CODE_OTEL_SHUTDOWN_TIMEOUT_MS
CLAUDE_CODE_PARENT_SESSION_ID
CLAUDE_CODE_PLAN_V
CLAUDE_CODE_PROFILE_QUERY
CLAUDE_CODE_PROFILE_STARTUP
CLAUDE_CODE_PROXY_RESOLVES_HOSTS
CLAUDE_CODE_REMOTE
CLAUDE_CODE_REMOTE_ENVIRONMENT_TYPE
CLAUDE_CODE_REMOTE_SESSION_ID
CLAUDE_CODE_SESSION_ACCESS_TOKEN
CLAUDE_CODE_SESSION_ID
CLAUDE_CODE_SHELL
CLAUDE_CODE_SHELL_PREFIX
CLAUDE_CODE_SKIP_BEDROCK_AUTH
CLAUDE_CODE_SKIP_FOUNDRY_AUTH
CLAUDE_CODE_SKIP_PROMPT_HISTORY
CLAUDE_CODE_SKIP_VERTEX_AUTH
CLAUDE_CODE_SSE_PORT
CLAUDE_CODE_SUBAGENT_MODEL
CLAUDE_CODE_SYNTAX_HIGHLIGHT
CLAUDE_CODE_TAGS
CLAUDE_CODE_TEAM_NAME
CLAUDE_CODE_WEBSOCKET_AUTH_FILE_DESCRIPTOR
CLAUDE_CONFIG_DIR
CLAUDE_DEBUG
CLAUDE_ENV_FILE
```

### Disable Flags
```
DISABLE_AUTO_MIGRATE_TO_NATIVE
DISABLE_AUTOUPDATER
DISABLE_BUG_COMMAND
DISABLE_COMPACT
DISABLE_COST_WARNINGS
DISABLE_DOCTOR_COMMAND
DISABLE_ERROR_REPORTING
DISABLE_EXTRA_USAGE_COMMAND
DISABLE_FEEDBACK_COMMAND
DISABLE_INSTALLATION_CHECKS
DISABLE_INSTALL_GITHUB_APP_COMMAND
DISABLE_INTERLEAVED_THINKING
DISABLE_LOGIN_COMMAND
DISABLE_LOGOUT_COMMAND
DISABLE_MICROCOMPACT
DISABLE_PROMPT_CACHING
DISABLE_PROMPT_CACHING_HAIKU
DISABLE_PROMPT_CACHING_OPUS
DISABLE_PROMPT_CACHING_SONNET
DISABLE_TELEMETRY
DISABLE_UPGRADE_COMMAND
```

### Enable Flags
```
ENABLE_BASH_ENV_VAR_MATCHING
ENABLE_BASH_WRAPPER_MATCHING
ENABLE_BETA_TRACING_DETAILED
ENABLE_BTW
ENABLE_CODE_GUIDE_SUBAGENT
ENABLE_ENHANCED_TELEMETRY_BETA
ENABLE_EXPERIMENTAL_MCP_CLI
ENABLE_INCREMENTAL_TUI
ENABLE_LSP_TOOL
ENABLE_MCP_CLI
ENABLE_MCP_CLI_ENDPOINT
ENABLE_MCP_LARGE_OUTPUT_FILES
ENABLE_SUBAGENT_ZOOM
ENABLE_TOOL_SEARCH
```

### OpenTelemetry
```
OTEL_EXPORTER_OTLP_ENDPOINT
OTEL_EXPORTER_OTLP_HEADERS
OTEL_EXPORTER_OTLP_INSECURE
OTEL_EXPORTER_OTLP_LOGS_PROTOCOL
OTEL_EXPORTER_OTLP_METRICS_PROTOCOL
OTEL_EXPORTER_OTLP_METRICS_TEMPORALITY_PREFERENCE
OTEL_EXPORTER_OTLP_PROTOCOL
OTEL_EXPORTER_OTLP_TRACES_PROTOCOL
OTEL_LOGS_EXPORTER
OTEL_LOGS_EXPORT_INTERVAL
OTEL_LOG_TOOL_CONTENT
OTEL_LOG_USER_PROMPTS
OTEL_METRIC_EXPORT_INTERVAL
OTEL_METRICS_EXPORTER
OTEL_TRACES_EXPORTER
OTEL_TRACES_EXPORT_INTERVAL
```

### MCP Configuration
```
MCP_TIMEOUT
MCP_TOOL_TIMEOUT
MCP_OAUTH_CALLBACK_PORT
MCP_SERVER_CONNECTION_BATCH_SIZE
MAX_MCP_OUTPUT_TOKENS
ENABLE_MCP_LARGE_OUTPUT_FILES
ENABLE_MCP_CLI
ENABLE_MCP_CLI_ENDPOINT
ENABLE_EXPERIMENTAL_MCP_CLI
USE_MCP_CLI_DIR
```

### Output Configuration
```
CLAUDE_CODE_MAX_OUTPUT_TOKENS
MAX_THINKING_TOKENS
SLASH_COMMAND_TOOL_CHAR_BUDGET
BASH_MAX_OUTPUT_LENGTH
```

### Network & Proxy
```
HTTP_PROXY
HTTPS_PROXY
NO_PROXY
CLAUDE_CODE_PROXY_RESOLVES_HOSTS
CLAUDE_CODE_CLIENT_CERT
CLAUDE_CODE_CLIENT_KEY
CLAUDE_CODE_CLIENT_KEY_PASSPHRASE
```

### Bash Tool
```
BASH_DEFAULT_TIMEOUT_MS
BASH_MAX_TIMEOUT_MS
CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR
CLAUDE_CODE_SHELL_PREFIX
```

---

## 20. Useful Patterns for Orchestration

### Pattern: Context Injection at Session Start

```json
{
  "hooks": {
    "SessionStart": [{
      "matcher": "startup",
      "hooks": [{
        "type": "command",
        "command": "python3 load_context.py"
      }]
    }]
  }
}
```

The hook script outputs JSON with `additionalContext` field:
```json
{
  "hookSpecificOutput": {
    "hookEventName": "SessionStart",
    "additionalContext": "Context loaded from external system..."
  }
}
```

### Pattern: Permission Automation

```json
{
  "hooks": {
    "PermissionRequest": [{
      "matcher": "Bash",
      "hooks": [{
        "type": "command",
        "command": "python3 auto_permission.py"
      }]
    }]
  }
}
```

Hook script reads stdin JSON and outputs permission decision:
```python
import json, sys
data = json.load(sys.stdin)
command = data.get("tool_input", {}).get("command", "")
if command.startswith("npm ") or command.startswith("cargo "):
    print(json.dumps({
        "hookSpecificOutput": {
            "hookEventName": "PermissionRequest",
            "decision": {"behavior": "allow"}
        }
    }))
    sys.exit(0)
sys.exit(1)  # Continue with normal permission flow
```

### Pattern: Tool Call Modification

PreToolUse hooks can modify tool inputs before execution:
```python
import json, sys
data = json.load(sys.stdin)
if data["tool_name"] == "Bash":
    command = data["tool_input"]["command"]
    # Add timeout wrapper
    data["tool_input"]["command"] = f"timeout 60 {command}"
    print(json.dumps({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "allow",
            "updatedInput": data["tool_input"]
        }
    }))
    sys.exit(0)
sys.exit(1)
```

### Pattern: Event Streaming to External System

```bash
# Start Claude Code with streaming output
claude -p "implement feature" --output-format stream-json 2>&1 | \
  while read line; do
    # Send each event to external system
    curl -X POST http://localhost:8080/events -d "$line"
  done
```

---

*Generated from official documentation and binary analysis of Claude Code v2.0.76*
*Last updated: 2025-12-28*

## Sources

- [Claude Code Hooks Reference](https://code.claude.com/docs/en/hooks)
- [Claude Code Settings](https://code.claude.com/docs/en/settings)
- [Agent Skills](https://code.claude.com/docs/en/skills)
- [Subagents](https://code.claude.com/docs/en/sub-agents)
- [Create Plugins](https://code.claude.com/docs/en/plugins)
- [CLI Reference](https://code.claude.com/docs/en/cli-reference)
- [VS Code Integration](https://code.claude.com/docs/en/vs-code)
- [Claude Code GitHub Repository](https://github.com/anthropics/claude-code)
- [GitHub Issues (Known Issues)](https://github.com/anthropics/claude-code/issues)
