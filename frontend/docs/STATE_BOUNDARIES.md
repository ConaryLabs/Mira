# Frontend State Management Boundaries

## Store Architecture

The Mira frontend uses Zustand for state management with 12 stores:

### 1. `useChatStore` - Chat & Messaging State
**Location**: `src/stores/useChatStore.ts`
**Responsibility**: Manages chat messages and streaming state
**Persisted**: Yes (localStorage)

**State**:
- `messages: ChatMessage[]` - Chat message history
- `currentSessionId: string` - Active session ID
- `isWaitingForResponse: boolean` - Waiting for assistant response
- `isStreaming: boolean` - Currently streaming a response
- `streamingContent: string` - Accumulated streaming content
- `streamingMessageId: string | null` - ID of streaming message

**Actions**:
- `addMessage()` - Add a new chat message
- `updateMessage()` - Update existing message
- `setMessages()` - Replace all messages
- `clearMessages()` - Clear message history
- `startStreaming()` - Begin streaming mode
- `appendStreamContent()` - Append streamed content
- `endStreaming()` - Finalize streaming message
- `setSessionId()` - Change session

**Hooks**:
- `useCurrentSession()` - Get messages and session ID

---

### 2. `useAppState` - Application Global State
**Location**: `src/stores/useAppState.ts`
**Responsibility**: Manages application-wide state (UI, projects, artifacts, connection)
**Persisted**: Partial (projects, artifacts, activeArtifactId only)

**State Domains**:

#### UI State (ephemeral)
- `showArtifacts: boolean` - Artifact panel visibility
- `showFileExplorer: boolean` - File explorer visibility
- `quickOpenVisible: boolean` - Quick open modal state

#### Project State (persisted)
- `currentProject: Project | null` - Active project
- `projects: Project[]` - All projects

#### Git State (ephemeral)
- `modifiedFiles: string[]` - Modified file paths
- `currentBranch: string` - Active git branch
- `gitStatus: any` - Git status data

#### Artifact State (persisted)
- `artifacts: Artifact[]` - Code artifacts from LLM
- `activeArtifactId: string | null` - Selected artifact
- `appliedFiles: Set<string>` - Applied artifact IDs

**Note**: These are **viewer artifacts** (files to preview/apply), distinct from ChatStore's message artifacts.

#### Connection State (ephemeral)
- `isReconnecting: boolean` - WebSocket reconnection status
- `reconnectAttempts: number` - Reconnection attempt count
- `connectionStatus: string` - Connection status message
- `connectionError: string | null` - Connection error

#### Rate Limiting State (ephemeral)
- `canSendMessage: boolean` - Message sending allowed
- `rateLimitUntil: number | null` - Rate limit expiry timestamp

#### Toast Notifications (ephemeral)
- `toasts: Toast[]` - Active toast notifications

#### Code Intelligence (ephemeral)
- `codeAnalysis: any` - Code analysis data
- `complexityHotspots: any[]` - Complexity hotspot data

#### Memory & Context (ephemeral)
- `relevantMemories: any[]` - Relevant memory entries
- `recentTopics: string[]` - Recent conversation topics

**Convenience Hooks**:
- `useProjectState()` - Get project-related state
- `useArtifactState()` - Get artifact-related state

---

### 3. `useWebSocketStore` - WebSocket Connection
**Location**: `src/stores/useWebSocketStore.ts`
**Responsibility**: Manages WebSocket connection and message routing
**Persisted**: No

**State**:
- WebSocket connection instance
- Connection status
- Message subscribers
- Send/receive queues

---

### 4. `useUIStore` - UI Preferences
**Location**: `src/stores/useUIStore.ts`
**Responsibility**: UI preferences and settings
**Persisted**: Likely yes

---

### 5. `useAuthStore` - Authentication
**Location**: `src/stores/useAuthStore.ts`
**Responsibility**: User authentication state
**Persisted**: Likely yes

---

### 6. `useCodeIntelligenceStore` - Code Intelligence Panel
**Location**: `src/stores/useCodeIntelligenceStore.ts`
**Responsibility**: Manages Intelligence Panel state (budget, search, build errors, etc.)
**Persisted**: No

**State**:
- `isPanelVisible: boolean` - Intelligence panel visibility
- `activeTab: 'budget' | 'search' | 'cochange' | 'builds' | 'tools' | 'expertise'` - Active tab
- `budget: BudgetStatus | null` - Budget usage data
- `semanticResults: SemanticSearchResult[]` - Semantic search results
- `cochangeSuggestions: CochangeSuggestion[]` - Co-change file suggestions
- `buildErrors: BuildError[]` - Build errors for project
- Loading states for each data type

**Actions**:
- `togglePanel()` - Show/hide intelligence panel
- `setActiveTab()` - Switch active tab
- `setBudget()` - Update budget status
- `setSemanticResults()` - Set semantic search results
- `setCochangeSuggestions()` - Set co-change suggestions
- `setBuildErrors()` - Set build errors

**Used By**:
- `IntelligencePanel.tsx` - Main container with tabs
- `BudgetTracker.tsx` - Budget usage display
- `SemanticSearch.tsx` - Code semantic search
- `CoChangeSuggestions.tsx` - File co-change patterns
- `BuildErrorsPanel.tsx` - Build errors and stats
- `ToolsDashboard.tsx` - Tool synthesis dashboard

---

### 7. `useActivityStore` - Operation Activity
**Location**: `src/stores/useActivityStore.ts`
**Responsibility**: Tracks current operation for activity indicator
**Persisted**: No

---

### 8. `useAgentStore` - Background Agents
**Location**: `src/stores/useAgentStore.ts`
**Responsibility**: Manages background Codex agent state
**Persisted**: No

**State**:
- `agents: Agent[]` - List of background agents
- `isPanelVisible: boolean` - Agent panel visibility

**Actions**:
- `addAgent()` - Add a new background agent
- `updateAgent()` - Update agent status/progress
- `removeAgent()` - Remove completed/failed agent
- `togglePanel()` - Show/hide agent panel

---

### 9. `useReviewStore` - Code Review Panel
**Location**: `src/stores/useReviewStore.ts`
**Responsibility**: Manages code review panel state
**Persisted**: No

**State**:
- `isPanelVisible: boolean` - Review panel visibility
- `loading: boolean` - Loading diff state
- `diff: string | null` - Current diff content
- `reviewTarget: ReviewTarget` - Target type (uncommitted, staged, branch, commit)
- `baseBranch: string` - Base branch for comparison
- `commitHash: string` - Specific commit hash
- `reviewResult: string | null` - LLM review result

**Actions**:
- `togglePanel()` - Show/hide review panel
- `setDiff()` - Update diff content
- `setReviewTarget()` - Change review target type
- `setReviewResult()` - Store LLM review

---

### 10. `useSudoStore` - Sudo Approval State
**Location**: `src/stores/useSudoStore.ts`
**Responsibility**: Manages sudo approval prompts
**Persisted**: No

**State**:
- `pendingApproval: SudoRequest | null` - Current pending sudo request
- `approvalHistory: SudoApproval[]` - Recent approvals

---

### 11. `useUsageStore` - Usage Tracking
**Location**: `src/stores/useUsageStore.ts`
**Responsibility**: Tracks API usage and budget
**Persisted**: No

---

### 12. `useThemeStore` - Theme Preferences
**Location**: `src/stores/useThemeStore.ts`
**Responsibility**: Manages dark/light mode theme
**Persisted**: Yes (localStorage)

**State**:
- `theme: 'light' | 'dark' | 'system'` - Current theme setting

---

## State Ownership Rules

### Artifacts: Two Types
1. **Message Artifacts** (ChatStore)
   - Attached to specific chat messages
   - Historical record of what was generated
   - Used for display in chat history

2. **Viewer Artifacts** (AppState)
   - Active code files to preview/edit/apply
   - Used by Artifact Viewer panel
   - Can be applied to project files

### Data Flow

```
Backend WebSocket Message
         ↓
useWebSocketMessageHandler
         ↓
  ┌─────────────┬──────────────┬─────────────────────┐
  ↓             ↓              ↓                     ↓
ChatStore   AppState      UIStore      CodeIntelligenceStore
(messages)  (artifacts)   (toasts)     (budget, search, builds)
```

### Update Patterns

1. **WebSocket Messages** → Multiple stores updated via hooks
2. **User Actions** → Direct store updates via actions
3. **Side Effects** → Handled in hooks, not in stores

---

## Simplification Opportunities

### Current Issues

1. **Overlapping Concerns**:
   - Connection state in both AppState and WebSocketStore
   - Toast management could be separate
   - Code Intelligence & Memory barely used

2. **Large Monolithic Store**:
   - `useAppState` has 8 different state domains
   - Makes testing difficult
   - Hard to reason about dependencies

3. **Unclear Boundaries**:
   - When to use AppState vs creating new store?
   - Connection state duplicated

### Recommended Simplifications

1. **Keep Current Structure** (Minimal Change)
   - Document boundaries clearly (this file)
   - Add JSDoc comments to stores
   - Remove unused state (codeAnalysis, complexityHotspots, relevantMemories, recentTopics)

2. **Moderate Refactoring** (Recommended)
   - Extract `useToastStore` from AppState
   - Move connection state to WebSocketStore only
   - Remove unused state domains
   - Keep artifacts in AppState (they're viewer-specific)

3. **Full Separation** (Over-engineering for current scale)
   - Separate stores: UI, Project, Git, Artifact, Toast, Connection
   - More files to manage
   - Overkill for current app size

### Decision: Moderate Refactoring

Remove unused state and clarify boundaries, but keep the core structure.

---

## Testing Strategy

### Unit Testing Stores

```typescript
// Example: Testing ChatStore
import { renderHook, act } from '@testing-library/react';
import { useChatStore } from './useChatStore';

test('adds message correctly', () => {
  const { result } = renderHook(() => useChatStore());

  act(() => {
    result.current.addMessage({
      id: 'test-1',
      role: 'user',
      content: 'Hello',
      timestamp: Date.now(),
    });
  });

  expect(result.current.messages).toHaveLength(1);
});
```

### Integration Testing

Test complete data flows through multiple stores using hooks.

---

## Migration Guide

If refactoring stores:

1. **Extract Toast Store**:
   ```typescript
   // Before
   useAppState().addToast({ type: 'success', message: 'Done!' });

   // After
   useToastStore().add({ type: 'success', message: 'Done!' });
   ```

2. **Consolidate Connection State**:
   ```typescript
   // Before
   useAppState().setConnectionStatus('connected');

   // After
   useWebSocketStore().setStatus('connected');
   ```

3. **Remove Unused State**:
   - Delete `codeAnalysis`, `complexityHotspots`
   - Delete `relevantMemories`, `recentTopics`
   - Clean up actions that set these values

---

## Best Practices

1. **Single Responsibility**: Each store should manage one domain
2. **No Business Logic**: Keep complex logic in hooks/services
3. **Explicit Actions**: Prefer specific actions over generic setters
4. **Type Safety**: Use TypeScript interfaces for all state
5. **Persistence**: Only persist user data, not ephemeral state
6. **Selectors**: Use Zustand selectors to minimize re-renders

---

## References

- [Zustand Documentation](https://github.com/pmndrs/zustand)
- [State Management Patterns](https://kentcdodds.com/blog/application-state-management-with-react)
