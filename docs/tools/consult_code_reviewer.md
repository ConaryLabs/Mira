# consult_code_reviewer

Consult the Code Reviewer expert to find bugs, quality issues, and improvement opportunities in code.

## Usage

```json
{
  "name": "consult_code_reviewer",
  "arguments": {
    "context": "Code to review",
    "question": "Optional specific aspect to focus on"
  }
}
```

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| context | String | Yes | The code, file path, or code snippet to review. Can include multiple files or a description of what to review. |
| question | String | No | Optional specific aspect to focus on (e.g., "check error handling", "review performance", "look for security issues"). |

## Returns

Returns a detailed code review analysis including:
- Issues categorized by severity (critical/major/minor/nit)
- Specific problems found with line numbers and function names where applicable
- Concrete fix suggestions
- Summary of findings

The response is formatted with markdown for readability. Findings are automatically parsed and stored in the review findings database for later tracking and learning.

## Examples

**Example 1: Review a specific function**
```json
{
  "name": "consult_code_reviewer",
  "arguments": {
    "context": "File: src/auth.rs\n\n```rust\nfn validate_token(token: &str) -> bool {\n    if token.len() < 10 {\n        return false;\n    }\n    \n    // TODO: Add proper validation\n    true\n}\n```",
    "question": "Check for security issues and missing validation"
  }
}
```

**Expected output:**
```
## Code Review Findings

### Critical Issues
1. **Incomplete token validation** - The function only checks length but doesn't validate token format or signature. This could allow invalid tokens to pass.

### Major Issues  
1. **Missing error handling** - No error messages or logging for failed validation.
2. **TODO comment** - Indicates incomplete implementation.

### Suggestions
1. Add proper cryptographic validation for JWT or similar tokens
2. Return Result<(), ValidationError> instead of bool for better error handling
3. Add logging for validation failures
4. Remove TODO comment once implemented
```

**Example 2: Review code with context**
```json
{
  "name": "consult_code_reviewer", 
  "arguments": {
    "context": "I'm working on a database connection pool. Here's the main connection manager:\n\n```rust\nimpl ConnectionPool {\n    pub fn get_connection(&self) -> Connection {\n        if self.connections.is_empty() {\n            self.create_connection()\n        } else {\n            self.connections.pop().unwrap()\n        }\n    }\n}\n```",
    "question": "Look for potential bugs and thread safety issues"
  }
}
```

**Expected output:**
```
## Code Review Findings

### Critical Issues
1. **Unwrap without error handling** - Line 7: `.unwrap()` will panic if `pop()` returns None, even though the `if` check should prevent this, but race conditions could occur in multi-threaded use.

### Major Issues
1. **Thread safety not guaranteed** - No synchronization primitives shown; if `ConnectionPool` is shared across threads, `connections` should be protected.
2. **Missing connection limits** - No maximum pool size check could lead to resource exhaustion.

### Suggestions
1. Use `expect()` with meaningful message instead of `unwrap()`
2. Add Mutex or RwLock around `connections` if shared
3. Implement maximum pool size and connection timeout
4. Consider using established pool library (r2d2, bb8)
```

## Errors

- **Timeout errors**: The consultation may time out after 10 minutes if the expert requires extensive tool usage or the LLM is slow to respond.
- **LLM provider errors**: If the configured LLM provider (DeepSeek, OpenAI, etc.) is unavailable or rate-limited.
- **Tool execution errors**: If the expert tries to use tools like `search_code` or `read_file` but they fail due to missing project context or file permissions.
- **Memory errors**: If the system cannot access learned patterns from past reviews due to database issues.

Common error messages:
- `"Expert consultation timed out after 600s"` - The consultation took too long
- `"Expert consultation failed: <provider error>"` - LLM provider issue
- `"No project context set"` - Expert tools require an active project
- `"Failed to get symbols: <error>"` - File analysis failed

## See Also

- [consult_experts](./consult_experts.md) - Consult multiple experts in parallel
- [consult_security](./consult_security.md) - Security-focused code review
- [list_findings](./list_findings.md) - View stored review findings
- [review_finding](./review_finding.md) - Accept/reject findings to improve future reviews
- [get_learned_patterns](./get_learned_patterns.md) - View patterns learned from past reviews