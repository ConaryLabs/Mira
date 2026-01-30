# llm/deepseek

DeepSeek Reasoner (V3.2) API client with tool calling support.

## Key Export

`DeepSeekClient` - Implements the `LlmClient` trait for the DeepSeek API.

## Sub-modules

| Module | Purpose |
|--------|---------|
| `client` | Client implementation |

## Features

- Chat completion with system/user/assistant messages
- Tool calling support for agentic expert workflows
- Uses the OpenAI-compatible request/response format via `llm/openai_compat`
