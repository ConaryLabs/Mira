# GitHub Issues to Create

This document lists technical debt and improvement opportunities identified during the codebase housecleaning. These should be created as GitHub issues for future work.

## Backend Issues

### 1. Add head filtering to QdrantMultiStore
**File**: `backend/src/memory/features/recall_engine/search/multihead_search.rs:57`
**Priority**: Medium
**Labels**: enhancement, memory-system

**Description**:
Add head filtering capability to QdrantMultiStore for more efficient multihead search operations.

**Context**:
```rust
// TODO: In the future, we might want to add head filtering to QdrantMultiStore
```

Currently, multihead search doesn't filter by head at the storage layer, which could be optimized.

---

### 2. Implement batch processing in message pipeline
**File**: `backend/src/memory/features/message_pipeline/mod.rs:86`
**Priority**: High
**Labels**: enhancement, performance, memory-system

**Description**:
Implement batch processing for message pipeline operations once storage layer is fully integrated.

**Context**:
```rust
// TODO: Implement batch processing once storage layer is integrated
```

Batch processing would improve performance for bulk message operations.

---

### 3. Add integration tests for git operations
**File**: Multiple in `backend/src/git/`
**Priority**: High
**Labels**: testing, git

**Description**:
Add comprehensive integration tests for git operations:
- Repository attachment
- Clone operations
- File operations
- Branch management
- Diff parsing

**Rationale**:
Git operations are critical and currently have limited test coverage.

---

### 4. Add unit tests for delegation tools
**File**: `backend/src/operations/delegation_tools.rs`
**Priority**: Medium
**Labels**: testing, operations

**Description**:
Add unit tests for tool schema generation and tool call parsing.

**Test cases**:
- Tool schema generation for each delegation tool
- Tool call parsing with various input formats
- Error handling for malformed tool calls

---

### 5. Add WebSocket message routing tests
**File**: `backend/src/api/ws/chat/message_router.rs`
**Priority**: Medium
**Labels**: testing, websocket

**Description**:
Add comprehensive tests for message router refactoring:
- Each command type handler
- Error handling paths
- Event conversion logic

**Rationale**:
Recent refactoring simplified the code but tests are needed to ensure correctness.

---

## Frontend Issues

### 6. Implement real authentication
**Files**:
- `frontend/src/stores/useAuthStore.ts:34`
- `frontend/src/config/app.ts:30,42`
**Priority**: High
**Labels**: feature, security, authentication

**Description**:
Replace placeholder auth implementation with real authentication system.

**Current state**:
```typescript
// TODO: Implement real auth
```

**Requirements**:
- User authentication flow
- Session management
- Token refresh
- Logout functionality
- Integration with backend auth endpoints

---

### 7. Add error toast notifications
**File**: `frontend/src/components/CommitPushButton.tsx:35`
**Priority**: Low
**Labels**: enhancement, ui, error-handling

**Description**:
Add user-facing error toast notifications when git operations fail.

**Context**:
```typescript
// TODO: Show error toast to user
```

Currently errors are logged but not shown to user.

---

### 8. Add unit tests for artifact utilities
**File**: `frontend/src/utils/artifact.ts`
**Priority**: High
**Labels**: testing, frontend

**Description**:
Add comprehensive unit tests for new artifact utility functions:
- `createArtifact()` - various input formats
- `extractArtifacts()` - different payload structures
- `normalizePath()` - path edge cases
- `extractContent()`, `extractPath()`, `extractLanguage()` - field extraction

**Rationale**:
These utilities were recently refactored from multiple handlers and need test coverage.

---

### 9. Add integration tests for state management
**File**: `frontend/src/stores/`
**Priority**: Medium
**Labels**: testing, frontend, state-management

**Description**:
Add integration tests for state store interactions:
- `useAppState` + `useChatStore` interactions
- WebSocket message flow through stores
- Persistence behavior
- State reset functionality

---

## Documentation Issues

### 10. Document WebSocket message protocol
**Priority**: Medium
**Labels**: documentation

**Description**:
Create comprehensive documentation for WebSocket message protocol:
- Client → Server message types
- Server → Client message types
- Streaming protocol
- Error handling
- Event types

**Location**: Create `docs/WEBSOCKET_PROTOCOL.md`

---

### 11. Document testing strategy
**Priority**: Medium
**Labels**: documentation, testing

**Description**:
Document testing approach and best practices:
- Unit test guidelines
- Integration test patterns
- Test data management
- Mocking strategies
- CI/CD integration

**Location**: Create `docs/TESTING.md`

---

### 12. Add API documentation
**Priority**: High
**Labels**: documentation, api

**Description**:
Generate or create API documentation for:
- REST endpoints (if any)
- WebSocket commands
- Request/response formats
- Error codes
- Rate limiting

**Location**: Create `docs/API.md`

---

## Performance Issues

### 13. Optimize bundle size
**File**: Frontend build output
**Priority**: Medium
**Labels**: performance, frontend

**Description**:
Frontend bundle is 1.08 MB (366 KB gzipped). Consider:
- Code splitting with dynamic imports
- Lazy loading routes
- Tree shaking optimization
- Analyze bundle with `rollup-plugin-visualizer`

**Current**:
```
dist/assets/index-CWwpVvMP.js   1,079.02 kB │ gzip: 366.23 kB
```

**Target**: < 500 KB initial bundle

---

### 14. Add database connection pooling tuning
**File**: `backend/src/state.rs`
**Priority**: Low
**Labels**: performance, database

**Description**:
Review and optimize SQLite connection pool settings:
- Max connections
- Idle timeout
- Connection lifetime
- Query timeout

Current settings may not be optimal for production load.

---

## Code Quality Issues

### 15. Reduce clippy warnings
**File**: Multiple files in backend
**Priority**: Low
**Labels**: code-quality, rust

**Description**:
Address pre-existing clippy warnings (117 warnings currently).

**Approach**:
- Categorize warnings by type
- Create separate issues for each category
- Fix highest-impact warnings first

---

## Security Issues

### 16. Add rate limiting configuration
**File**: `backend/src/config/`
**Priority**: High
**Labels**: security, configuration

**Description**:
Add configurable rate limiting:
- Per-session limits
- Global limits
- Burst allowance
- Backoff strategy

Currently hardcoded in application logic.

---

### 17. Add input validation middleware
**File**: `backend/src/api/`
**Priority**: High
**Labels**: security, validation

**Description**:
Create middleware for input validation:
- Size limits
- Content type validation
- Schema validation
- SQL injection prevention
- XSS prevention

---

## Nice-to-Have Enhancements

### 18. Add telemetry and metrics
**Priority**: Low
**Labels**: enhancement, observability

**Description**:
Add telemetry for:
- Operation success/failure rates
- Response times
- Memory usage
- Active connections
- Cache hit rates

Consider using OpenTelemetry.

---

### 19. Add database migration rollback tests
**File**: `backend/migrations/`
**Priority**: Low
**Labels**: testing, database

**Description**:
Test that all migrations can be rolled back without data loss.

---

### 20. Create development environment setup guide
**Priority**: Medium
**Labels**: documentation, developer-experience

**Description**:
Comprehensive guide for new developers:
- Prerequisites
- Environment setup
- Running locally
- Testing
- Contributing guidelines

**Location**: Create `docs/DEVELOPMENT.md`

---

## Issue Creation Checklist

When creating these issues on GitHub:

- [ ] Use appropriate labels (enhancement, bug, documentation, etc.)
- [ ] Set priority (High, Medium, Low)
- [ ] Add milestone if applicable
- [ ] Link to related issues
- [ ] Include code snippets and file references
- [ ] Add acceptance criteria
- [ ] Estimate effort (optional)

---

## Summary

**Total Issues**: 20
- Backend: 5
- Frontend: 4
- Documentation: 3
- Performance: 2
- Code Quality: 1
- Security: 2
- Nice-to-Have: 3

**Priority Breakdown**:
- High: 7 issues
- Medium: 9 issues
- Low: 4 issues
