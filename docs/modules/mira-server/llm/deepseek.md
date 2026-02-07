# llm/deepseek

DeepSeek API client with tool calling support. Used for expert consultations and background intelligence tasks.

## Key Export

`DeepSeekClient` — Implements the `LlmClient` trait for the DeepSeek API.

## Features

- Chat completion with system/user/assistant messages
- Tool calling support for agentic expert workflows
- Uses the OpenAI-compatible request/response format via `llm/openai_compat`

## Chat vs Reasoner Split

The `ProviderFactory` creates two separate DeepSeek clients when the primary model is a reasoner:

- **`deepseek-chat`** — Handles tool-calling loops (agentic exploration). Max tokens: 8192.
- **`deepseek-reasoner`** — Handles final synthesis. Max tokens: 65536.

This split is managed by `ReasoningStrategy::Decoupled` in `tools/core/experts/strategy.rs`. It prevents OOM from unbounded `reasoning_content` accumulation during long multi-turn tool loops, where the reasoner model would append internal chain-of-thought to every response.

When no reasoner model is detected, a `ReasoningStrategy::Single` is used instead.
