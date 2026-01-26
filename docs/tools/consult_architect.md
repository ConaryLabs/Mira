# consult_architect

Consult the Architect expert for system design and architectural decisions.

## Usage

```json
{
  "name": "consult_architect",
  "arguments": {
    "context": "Code, design, or situation to analyze",
    "question": "Specific question to answer (optional)"
  }
}
```

## Parameters

| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| context | string | Yes | Code, design, or situation to analyze. Provide relevant code snippets, architecture diagrams, or system descriptions. |
| question | string | No | Specific question to answer. If not provided, the architect will analyze the context and provide general architectural feedback. |

## Returns

Returns a detailed architectural analysis with recommendations, tradeoffs, and refactoring strategies. The response includes:

1. Key recommendations prioritized by impact
2. Reasoning with specific references to the provided context
3. Alternative approaches with clear tradeoffs
4. Prioritized issues by severity and impact

The architect has access to tools for exploring the codebase (search_code, get_symbols, read_file, find_callers, find_callees, recall) and will use them as needed to gather additional context.

## Examples

**Example 1: Analyzing a microservice design**
```json
{
  "name": "consult_architect",
  "arguments": {
    "context": "I'm designing a payment processing system with three services: payment-gateway (handles external APIs), transaction-processor (handles business logic), and notification-service (sends emails/SMS). Each service has its own database. The payment-gateway needs to call transaction-processor which then calls notification-service.",
    "question": "What are the architectural risks and how should I handle error propagation between services?"
  }
}
```

**Expected output:**
```
## Architectural Analysis: Payment Processing System

**Key Recommendation:** Implement circuit breakers and dead letter queues for inter-service communication.

**Issues Identified:**
1. **Cascading failures risk** - Payment-gateway → transaction-processor → notification-service creates a failure chain
2. **Data consistency** - Each service having its own database requires distributed transactions or eventual consistency
3. **Error visibility** - Failures in notification-service shouldn't block payment processing

**Alternatives with Tradeoffs:**
1. **Synchronous with circuit breakers** (current approach)
   - Pros: Simple to implement, immediate feedback
   - Cons: Tight coupling, cascading failures

2. **Event-driven with message queue**
   - Pros: Decouples services, better fault tolerance
   - Cons: Eventual consistency, more complex to debug

3. **Saga pattern with compensation**
   - Pros: Maintains data consistency across services
   - Cons: Complex implementation, requires rollback logic

**Prioritized Actions:**
1. HIGH: Add circuit breakers between all service calls
2. HIGH: Implement dead letter queues for failed notifications
3. MEDIUM: Consider event-driven architecture for non-critical paths
4. LOW: Add distributed tracing for better observability
```

**Example 2: Code architecture review**
```json
{
  "name": "consult_architect", 
  "arguments": {
    "context": "Here's my Rust application structure:\n\nsrc/\n├── main.rs\n├── api/\n│   ├── mod.rs\n│   ├── routes.rs\n│   └── middleware.rs\n├── db/\n│   ├── mod.rs\n│   ├── models.rs\n│   └── queries.rs\n├── services/\n│   ├── mod.rs\n│   ├── payment.rs\n│   └── notification.rs\n└── utils/\n    ├── mod.rs\n    └── validation.rs\n\nThe api module depends on services, which depend on db. Everything depends on utils.",
    "question": "Is this dependency structure appropriate? Should I consider a different architectural pattern?"
  }
}
```

**Expected output (truncated):**
```
## Architectural Analysis: Application Structure

**Key Recommendation:** Consider implementing a cleaner dependency inversion with interfaces/traits.

**Current Structure Issues:**
1. **Circular dependency risk** - If utils needs something from api/services/db
2. **Tight coupling** - api directly depends on concrete service implementations
3. **Testing difficulty** - Hard to mock dependencies in current structure

**Recommended Refactoring:**
1. Define traits/interfaces in a `domain/` module
2. Move business logic from services to domain
3. Make api depend on domain traits, not concrete services
4. Implement services as adapters that satisfy domain traits

**Alternative Patterns:**
1. **Hexagonal/Ports & Adapters** - Isolates core domain from infrastructure
2. **Clean Architecture** - Explicit dependency rules with use cases
3. **Current layered architecture** - Simple but becomes rigid over time

**Tradeoffs:** Hexagonal adds complexity but improves testability and maintainability.
```

## Errors

The following errors may occur:

1. **Timeout errors** - The consultation may time out after 10 minutes if the architect requires extensive tool usage or the LLM provider is slow. Error message: `"Architect consultation timed out after 600s"`

2. **LLM provider errors** - If the configured LLM provider (DeepSeek, OpenAI, etc.) fails or returns an error. Error message: `"Expert consultation failed: <provider error>"`

3. **Tool execution errors** - If the architect tries to use a tool (like search_code or read_file) and it fails. The error will be included in the response.

4. **Maximum iterations exceeded** - If the architect makes more than 100 tool calls (prevents infinite loops). Error message: `"Expert exceeded maximum iterations (100). Partial analysis may be available."`

5. **Missing project context** - If the architect needs to access project files but no project is set. The tool will still work but may have limited context.

## See Also

- `consult_plan_reviewer` - Validate implementation plans
- `consult_scope_analyst` - Find missing requirements and edge cases  
- `consult_code_reviewer` - Find bugs and quality issues
- `consult_security` - Identify vulnerabilities
- `consult_experts` - Consult multiple experts in parallel
- `configure_expert` - Customize expert prompts and providers