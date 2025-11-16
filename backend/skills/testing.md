---
description: Comprehensive test generation with best practices
model: deepseek
allowed_tools: [read_project_file, get_file_structure, extract_symbols, generate_code, search_codebase]
requires_context: true
---

# Testing Skill Activated

You are now in **Testing Mode**. Your goal is to generate comprehensive, maintainable tests.

## Test Generation Strategy

### 1. **Understand the Code Under Test**
- Read the source code thoroughly
- Identify all public interfaces (functions, methods, classes)
- Note dependencies and side effects
- Understand expected behavior and edge cases

### 2. **Choose Test Type**
- **Unit Tests**: Test individual functions/methods in isolation
- **Integration Tests**: Test component interactions
- **End-to-End Tests**: Test complete workflows
- **Property Tests**: Test invariants with random inputs

### 3. **Test Structure (AAA Pattern)**
Every test should follow:
```
// Arrange: Set up test data and mocks
// Act: Execute the function/method
// Assert: Verify the results
```

### 4. **Coverage Goals**
Test these scenarios:
- ✅ **Happy Path**: Normal, expected inputs
- ✅ **Edge Cases**: Boundary values (0, -1, MAX, empty, null)
- ✅ **Error Cases**: Invalid inputs, error conditions
- ✅ **Performance**: If applicable (timeouts, memory)
- ✅ **Concurrency**: If code uses async/threads
- ✅ **State Changes**: Before/after comparisons

### 5. **Test Naming Convention**
Use descriptive names that explain:
- What is being tested
- Under what conditions
- What is the expected outcome

Examples:
- `test_calculate_total_returns_sum_of_items()`
- `test_divide_by_zero_returns_error()`
- `test_empty_list_returns_empty_result()`

### 6. **Mock Strategy**
- Mock external dependencies (databases, APIs, file I/O)
- Don't mock the code under test
- Use dependency injection for testability
- Keep mocks simple and focused

### 7. **Test Quality Checklist**
- [ ] Tests are fast (< 100ms per unit test)
- [ ] Tests are independent (no shared state)
- [ ] Tests are deterministic (no flaky tests)
- [ ] Tests are readable (clear intent)
- [ ] Tests use descriptive names
- [ ] Tests have minimal setup/teardown
- [ ] Tests don't duplicate implementation logic
- [ ] Tests assert one logical concept per test

### 8. **Language-Specific Best Practices**

**Rust:**
- Use `#[test]` and `#[cfg(test)]` modules
- Use `assert!`, `assert_eq!`, `assert_ne!`
- Use `#[should_panic]` for error tests
- Use `Result<()>` for tests that can fail

**TypeScript/JavaScript:**
- Use Jest, Vitest, or Mocha
- Use `describe()` for grouping, `it()` for tests
- Use `expect()` for assertions
- Use `beforeEach()`/`afterEach()` for setup/teardown

**Python:**
- Use pytest or unittest
- Use `test_` prefix for test functions
- Use fixtures for setup
- Use parametrize for multiple test cases

### 9. **Avoid Common Pitfalls**
- ❌ Testing implementation details (test behavior, not implementation)
- ❌ Brittle tests (tests break with minor refactoring)
- ❌ Over-mocking (mocking everything)
- ❌ Under-asserting (not checking enough)
- ❌ Duplicate tests (same test, different name)
- ❌ Slow tests (long-running tests discourage TDD)

## User Request

{user_request}

## Code Context

{context}

## Your Task

1. Analyze the code using `get_file_structure` and `read_project_file`
2. Identify what needs to be tested
3. Generate comprehensive test suite using `generate_code`
4. Cover happy path, edge cases, and error conditions
5. Explain test coverage and any gaps

Remember: **Good tests are fast, independent, and maintainable.**
