# Claude Code Internal Architecture Map

**Version:** 2.0.76
**Build Date:** 2025-12-22T23:56:23Z
**Binary Location:** `/home/peter/.local/share/claude/versions/2.0.76`
**Type:** Native ELF 64-bit executable (bundled Node.js/JS)

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

### Hook Event Tracking
```javascript
function R8D(H, $, A, L) {
  // Tracks hook events:
  // - hook_event: event type
  // - hook_name: hook identifier
  // - num_hooks: count of hooks
  // - hook_definitions: hook configs
}
```

---

## 7. Integration Points for Orchestrator

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

### Note on CLAUDE_CODE_SSE_PORT
Despite appearing in some documentation, `CLAUDE_CODE_SSE_PORT` does **NOT** create a local
SSE server. Testing confirms:
- Setting this env var does not open any listening ports
- The string is not present in the compiled binary (v2.0.76)
- Claude Code only makes outbound HTTPS connections to Anthropic's API

**Correct approach for real-time events:** Use `--output-format stream-json` and parse stdout.
This provides JSON-formatted streaming events for all Claude Code activity.

---

## 8. Undocumented/Internal Features

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

---

## 9. Complete Environment Variable Reference

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

---

*Generated from binary analysis of Claude Code v2.0.76*
