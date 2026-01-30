# find_callees

Find all functions called by a given function. Uses the indexed call graph to trace outgoing calls from a specific function.

## Usage

```json
{
  "name": "find_callees",
  "arguments": {
    "function_name": "process_request",
    "limit": 20
  }
}
```

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| function_name | String | Yes | Function name to find callees for |
| limit | Integer | No | Max results to return (default: 20) |

## Returns

Formatted list of functions called by the target function, with file locations and line numbers.

If no callees are found: `No callees found for 'function_name'.`

## Examples

**Example 1: Find what a function calls**
```json
{
  "name": "find_callees",
  "arguments": { "function_name": "handle_login" }
}
```

**Example 2: With limit**
```json
{
  "name": "find_callees",
  "arguments": { "function_name": "main", "limit": 50 }
}
```

## Errors

- **"function_name is required"**: An empty function name was provided.
- **No results**: The function may not be indexed yet. Run `index(action="project")` first.

## See Also

- **find_callers**: Find all functions that call a given function (reverse direction)
- **get_symbols**: List symbols in a file
- **index**: Index the project to build the call graph
