# summarize_codebase

Generate summaries for codebase modules. Identifies modules that lack descriptions and generates concise summaries based on their code content, using LLM when available or heuristic analysis as a fallback.

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

### With LLM Provider
1. Queries the code database for modules without summaries
2. Reads code previews for each module from disk
3. Sends the code to an LLM (DeepSeek or Gemini) for summarization
4. Stores the generated summaries in the code database
5. Tracks LLM token usage

### Without LLM Provider (Heuristic Fallback)
1. Queries the code database for modules without summaries
2. Generates summaries from module metadata: file count, language distribution, top-level symbols
3. Summaries are prefixed with `[heuristic]` to distinguish from LLM-generated ones
4. Modules are kept upgradeable — when an LLM becomes available, they can be re-summarized for richer descriptions

This is also run automatically after `index(action="project")` completes.

## Prerequisites

- Active project context
- Project must be indexed first
- LLM provider recommended but not required (heuristic fallback available)

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
- **Database errors**: Failed to query or update module data.

## See Also

- **index**: Index the project (automatically triggers summarization)
- **project**: Initialize project context (codebase map uses module summaries)
- **configure_expert**: Configure which LLM provider is used
