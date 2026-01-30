# summarize_codebase

Generate LLM-powered summaries for codebase modules. Identifies modules that lack descriptions and uses an LLM to generate concise summaries based on their code content.

## Usage

```json
{
  "name": "summarize_codebase",
  "arguments": {}
}
```

## Parameters

None. Operates on the current active project.

## Returns

- **Summaries generated**: `Summarized 3 modules: src/handlers: HTTP request handlers...`
- **All done**: `All modules already have summaries.`

## Behavior

1. Queries the code database for modules without summaries
2. Reads code previews for each module from disk
3. Sends the code to an LLM (DeepSeek or Gemini) for summarization
4. Stores the generated summaries in the code database
5. Tracks LLM token usage

This is also run automatically after `index(action="project")` completes.

## Prerequisites

- Active project context
- At least one LLM provider configured (`DEEPSEEK_API_KEY` or `GEMINI_API_KEY`)
- Project must be indexed first

## Examples

**Example 1: Generate summaries**
```json
{
  "name": "summarize_codebase",
  "arguments": {}
}
```

## Errors

- **"No active project"**: Requires an active project context.
- **No LLM available**: Requires a configured LLM provider for generating summaries.
- **Database errors**: Failed to query or update module data.

## See Also

- **index**: Index the project (automatically triggers summarization)
- **project**: Initialize project context (codebase map uses module summaries)
- **configure_expert**: Configure which LLM provider is used
