# configure_expert

Configure expert system prompts, providers, and models. Customize how each expert role behaves when consulted.

## Usage

```json
{
  "name": "configure_expert",
  "arguments": {
    "action": "set",
    "role": "architect",
    "provider": "deepseek"
  }
}
```

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| action | String | Yes | Action to perform: `set`, `get`, `delete`, `list`, or `providers` |
| role | String | Conditional | Expert role (required for `set`, `get`, `delete`): `architect`, `plan_reviewer`, `scope_analyst`, `code_reviewer`, `security` |
| prompt | String | No | Custom system prompt (for `set`) |
| provider | String | No | LLM provider (for `set`): `deepseek` or `gemini` |
| model | String | No | Custom model name (for `set`, e.g. `deepseek-reasoner`, `gemini-2.0-flash-exp`) |

### Actions

| Action | Description | Required Params |
|--------|-------------|-----------------|
| `set` | Set or update configuration for an expert role | `action`, `role`, plus at least one of `prompt`/`provider`/`model` |
| `get` | Show current configuration for a role | `action`, `role` |
| `delete` | Remove custom configuration, reverting to defaults | `action`, `role` |
| `list` | List all custom configurations | `action` |
| `providers` | List available LLM providers and their default models | `action` |

## Returns

### `set`

```
Configuration updated for 'architect' expert: prompt set provider=deepseek
```

### `get`

```
Configuration for 'architect' (Software Architect):
  Provider: deepseek
  Model: deepseek-reasoner (default)
  Custom prompt: You are a software architect...
```

### `delete`

```
Configuration deleted for 'architect'. Reverted to defaults.
```

### `list`

```
2 expert configurations:

  architect: provider=deepseek, model=deepseek-reasoner, prompt=You are a...
  security: provider=gemini, model=default, prompt=(default)
```

Or: `No custom configurations. All experts use default settings.`

### `providers`

```
2 LLM providers available:

  deepseek: model=deepseek-reasoner (default)
  gemini: model=gemini-2.0-flash-exp

Set DEFAULT_LLM_PROVIDER env var to change the global default.
```

Or: `No LLM providers available. Set DEEPSEEK_API_KEY or GEMINI_API_KEY.`

## Examples

**Example 1: Set a custom provider for the architect role**
```json
{
  "name": "configure_expert",
  "arguments": { "action": "set", "role": "architect", "provider": "gemini" }
}
```

**Example 2: Check available providers**
```json
{
  "name": "configure_expert",
  "arguments": { "action": "providers" }
}
```

**Example 3: Reset a role to defaults**
```json
{
  "name": "configure_expert",
  "arguments": { "action": "delete", "role": "code_reviewer" }
}
```

## Errors

- **"role is required"**: The `set`, `get`, and `delete` actions require a `role`.
- **"Unknown expert role"**: The role must be one of the valid role keys.
- **"Unknown provider"**: The provider must be `deepseek` or `gemini`.
- **"At least one of prompt, provider, or model required"**: The `set` action needs something to set.

## See Also

- **consult_experts**: Run expert consultations using these configurations
- **usage**: Track LLM usage and costs from expert consultations
