# llm/deepseek

DeepSeek API client. Used for background intelligence tasks (summaries, pondering, diff analysis).

## Key Export

`DeepSeekClient` — Implements the `LlmClient` trait for the DeepSeek API.

## Features

- Chat completion with system/user/assistant messages
- `with_model()` constructor for model override
- `max_tokens_for_model()` with per-model limits
- `calculate_cache_hit_ratio()` for cache statistics
- Context budget management (110K tokens)
- Tool calling support
- Uses the OpenAI-compatible request/response format via `llm/openai_compat`
- Implements `LlmClient` trait: `provider_type()`, `model_name()`, `context_budget()`
