# consult_security

Consult the Security Analyst expert to identify vulnerabilities in code or system designs.

## Usage

```json
{
  "name": "consult_security",
  "arguments": {
    "context": "Code or design to analyze for security",
    "question": "Optional specific security concern"
  }
}
```

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| context | String | Yes | Code, design document, or system description to analyze for security vulnerabilities |
| question | String | No | Optional specific security concern or question to focus the analysis |

## Returns

Returns a detailed security analysis with findings categorized by severity (critical/high/medium/low). The response includes:
- List of security vulnerabilities identified
- Attack vectors and likelihood/impact assessment  
- Secure coding practice violations
- Remediation recommendations for each finding

Security findings are automatically parsed and stored in the code review findings database for tracking and learning.

## Examples

**Example 1: Analyzing authentication code**
```json
{
  "context": "Here's our authentication middleware:\n\n```rust\nfn validate_token(token: &str) -> bool {\n    // Simple string comparison\n    token == \"hardcoded_secret_token\"\n}\n```",
  "question": "Check for authentication vulnerabilities"
}
```

**Expected output:**
```
## Security Analysis - Authentication Code

### Critical Findings:

1. **[!!] Hardcoded Secret Token**
   - **Vulnerability**: Static secret token in source code
   - **Impact**: Token compromise leads to complete system breach
   - **Attack Vector**: Source code exposure, repository leaks
   - **Remediation**: Use environment variables, secrets management system

2. **[!] Weak Token Validation**
   - **Vulnerability**: Simple string comparison without timing-safe comparison
   - **Impact**: Timing attacks could reveal token
   - **Attack Vector**: Network timing analysis
   - **Remediation**: Use constant-time comparison functions

### Recommendations:
- Implement proper JWT or OAuth2 token validation
- Store secrets in environment variables or secure vault
- Add rate limiting to authentication endpoints
```

**Example 2: Checking SQL injection vulnerabilities**
```json
{
  "context": "Database query function:\n\n```python\ndef get_user(username):\n    query = f\"SELECT * FROM users WHERE username = '{username}'\"\n    return db.execute(query).fetchone()\n```",
  "question": "Identify SQL injection risks"
}
```

**Expected output:**
```
## Security Analysis - Database Queries

### Critical Findings:

1. **[!!] SQL Injection Vulnerability**
   - **Vulnerability**: Direct string interpolation in SQL query
   - **Impact**: Full database compromise, data exfiltration
   - **Attack Vector**: Malicious username input like `' OR '1'='1`
   - **Remediation**: Use parameterized queries or prepared statements

### Secure Coding Practices Violated:
- OWASP A1:2017 - Injection
- CWE-89: SQL Injection

### Remediation Example:
```python
def get_user(username):
    query = \"SELECT * FROM users WHERE username = ?\"
    return db.execute(query, (username,)).fetchone()
```
```

## Errors

- **Timeout Error**: `"Security consultation timed out after 600s"` - The expert analysis exceeded the 10-minute timeout limit
- **LLM Provider Error**: `"Expert consultation failed: <provider error>"` - The underlying LLM provider failed to respond
- **Tool Execution Error**: `"Search failed: <error>"` - One of the expert's tool calls (search_code, read_file, etc.) failed
- **Iteration Limit**: `"Expert exceeded maximum iterations (100). Partial analysis may be available."` - The expert agent loop exceeded iteration limits
- **No Project Context**: May fail if no active project is set (required for file-based tool access)

## See Also

- [`consult_architect`](consult_architect.md) - Consult the Architect expert for system design decisions
- [`consult_code_reviewer`](consult_code_reviewer.md) - Consult the Code Reviewer expert for code quality issues
- [`consult_experts`](consult_experts.md) - Consult multiple experts in parallel
- [`configure_expert`](configure_expert.md) - Configure expert system prompts and providers
- [`list_findings`](list_findings.md) - List security findings from expert consultations