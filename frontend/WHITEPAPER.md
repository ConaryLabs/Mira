# Mira Frontend Technical Whitepaper

This document provides a comprehensive technical reference for the Mira frontend architecture, designed to help LLMs understand how the system works.

**Version:** 0.9.0
**Last Updated:** December 10, 2025
**Framework:** React 18.2 + TypeScript 5.2
**Source Files:** 85+

---

## Table of Contents

1. [System Overview](#1-system-overview)
2. [Tech Stack & Dependencies](#2-tech-stack--dependencies)
3. [Directory Structure](#3-directory-structure)
4. [Zustand Stores](#4-zustand-stores)
5. [Custom React Hooks](#5-custom-react-hooks)
6. [Components](#6-components)
7. [Type Definitions](#7-type-definitions)
8. [WebSocket Communication](#8-websocket-communication)
9. [Routes & Pages](#9-routes--pages)
10. [State Flow Diagram](#10-state-flow-diagram)
11. [Data Persistence](#11-data-persistence)
12. [Configuration](#12-configuration)
13. [Component Dependency Map](#13-component-dependency-map)
14. [Testing Infrastructure](#14-testing-infrastructure)
15. [Theme Support](#15-theme-support)

---

## 1. System Overview

Mira frontend is a React single-page application that:

- Connects to the Rust backend via WebSocket (port 3001)
- Uses Zustand for state management (13 stores)
- Features real-time streaming responses
- Provides code editing via Monaco Editor
- Supports artifact management (code snippets)
- Includes comprehensive operation activity tracking

### Key Features

- **Real-time chat**: Streaming LLM responses with token-by-token display
- **Artifact viewer**: Monaco editor for code viewing/editing
- **Activity panel**: Operation tracking, tasks, tool executions
- **Intelligence panel**: Budget tracking, semantic search, co-change suggestions
- **Background agents**: Monitor Codex sessions
- **Code review**: Diff viewer with review functionality
- **Session management**: Create, resume, fork sessions
- **Project management**: Attach directories, manage repositories

---

## 2. Tech Stack & Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| React | 18.2 | UI framework |
| TypeScript | 5.2 | Type safety |
| Zustand | 5.0.8 | State management |
| React Router | 7.9.6 | Client-side routing |
| Vite | 7.1.4 | Build tool |
| TailwindCSS | 3.4.1 | Styling |
| Monaco Editor | 4.7.0 | Code editor |
| Lucide React | - | Icons |
| react-syntax-highlighter | - | Code highlighting |
| react-virtuoso | - | Virtualized lists |
| Vitest | - | Unit testing |
| React Testing Library | - | Component testing |

---

## 3. Directory Structure

```
frontend/src/
├── components/              # React UI components (49+ files)
│   ├── ActivitySections/    # Activity panel subsections
│   │   ├── PlanDisplay.tsx
│   │   ├── TasksSection.tsx
│   │   ├── ToolExecutionsSection.tsx
│   │   └── ReasoningSection.tsx
│   ├── documents/           # Document management
│   │   ├── DocumentsView.tsx
│   │   ├── DocumentUpload.tsx
│   │   ├── DocumentSearch.tsx
│   │   ├── DocumentList.tsx
│   │   └── DocumentsModal.tsx
│   ├── modals/              # Modal dialogs
│   │   └── DeleteConfirmModal.tsx
│   │
│   ├── ActivityPanel.tsx    # Right panel - operation tracking
│   ├── ArtifactPanel.tsx    # Code artifact viewer
│   ├── ArtifactToggle.tsx   # Show/hide artifacts button
│   ├── BackgroundAgentsPanel.tsx  # Codex session monitor
│   ├── BuildErrorsPanel.tsx # Build error tracking
│   ├── BudgetTracker.tsx    # Budget usage display
│   ├── ChangePasswordModal.tsx
│   ├── ChatArea.tsx         # Main chat container
│   ├── ChatInput.tsx        # Message input
│   ├── ChatMessage.tsx      # Individual message
│   ├── CoChangeSuggestions.tsx
│   ├── CodeBlock.tsx        # Inline code snippet
│   ├── CodebaseAttachModal.tsx
│   ├── ConnectionBanner.tsx # Connection status
│   ├── CreateProjectModal.tsx
│   ├── FileBrowser.tsx      # File tree navigator
│   ├── Header.tsx           # Top navigation bar
│   ├── IntelligencePanel.tsx # Code intelligence panel
│   ├── MessageList.tsx      # Chat message list
│   ├── MonacoEditor.tsx     # Code editor
│   ├── OpenDirectoryModal.tsx
│   ├── PermissionsPanel.tsx # Sudo permissions
│   ├── PlanDisplay.tsx      # Operation plan display
│   ├── PrivateRoute.tsx     # Auth protection
│   ├── ProjectSettingsModal.tsx
│   ├── ProjectsView.tsx     # Project management
│   ├── ReviewPanel.tsx      # Code review modal
│   ├── SemanticSearch.tsx   # Semantic code search
│   ├── SessionsModal.tsx    # Session management
│   ├── SudoApprovalInline.tsx
│   ├── TaskTracker.tsx      # Task status display
│   ├── ThinkingIndicator.tsx
│   ├── ToastContainer.tsx   # Notifications
│   ├── ToolsDashboard.tsx   # Available tools
│   ├── UnifiedDiffView.tsx  # Git diff viewer
│   └── UsageIndicator.tsx   # Usage/cost display
│
├── stores/                  # Zustand state management (13 stores)
│   ├── useChatStore.ts      # Messages, streaming, artifacts
│   ├── useWebSocketStore.ts # WebSocket connection
│   ├── useAppState.ts       # UI state, projects, artifacts
│   ├── useAuthStore.ts      # Authentication
│   ├── useCodeIntelligenceStore.ts
│   ├── useAgentStore.ts     # Background agents
│   ├── useActivityStore.ts  # Operation tracking
│   ├── useSudoStore.ts      # Sudo permissions
│   ├── useUsageStore.ts     # LLM usage tracking
│   ├── useThemeStore.ts     # Light/dark theme
│   ├── useReviewStore.ts    # Code review state
│   └── useUIStore.ts        # General UI state
│
├── hooks/                   # Custom React hooks (14 files)
│   ├── useWebSocketMessageHandler.ts  # Global message dispatcher
│   ├── useMessageHandler.ts           # Chat response handler
│   ├── useChatMessaging.ts            # Send chat messages
│   ├── useChatPersistence.ts          # Load/persist history
│   ├── useArtifacts.ts                # Artifact management
│   ├── useProjectOperations.ts        # Project CRUD
│   ├── useGitOperations.ts            # Git operations
│   ├── useSessionOperations.ts        # Session management
│   ├── useErrorHandler.ts             # Error handling
│   ├── useConnectionTracking.ts       # Connection state sync
│   ├── useCodeIntelligenceHandler.ts  # Code intel responses
│   ├── useArtifactFileContentWire.ts  # File → artifact bridge
│   └── useToolResultArtifactBridge.ts # Tool → artifact bridge
│
├── pages/                   # Route pages
│   └── Login.tsx            # Login page
│
├── types/                   # TypeScript definitions
│   └── index.ts             # All type exports
│
├── utils/                   # Utility functions
│   ├── artifact.ts          # Artifact creation/parsing
│   └── language.ts          # Language detection
│
├── config/                  # Configuration
│   └── app.ts               # App constants
│
├── test/                    # Test setup
│   └── setup.ts             # Test configuration
│
├── App.tsx                  # Root component with routing
├── Home.tsx                 # Main application layout
├── main.tsx                 # Entry point
└── index.css                # Global styles
```

---

## 4. Zustand Stores

### 4.1 useChatStore

**Purpose:** Chat messages, streaming state, artifacts

**Location:** `/frontend/src/stores/useChatStore.ts`

```typescript
interface ChatStoreState {
  messages: ChatMessage[];
  currentSessionId: string;
  isWaitingForResponse: boolean;
  isStreaming: boolean;
  streamingContent: string;
  streamingMessageId: string | null;
}

interface ChatMessage {
  id: string;
  role: 'user' | 'assistant' | 'system' | 'error';
  content: string;
  timestamp: number;
  artifacts?: Artifact[];
  tasks?: Task[];
  plan?: Plan;
  toolExecutions?: ToolExecution[];
  metadata?: MessageMetadata;
}

interface Artifact {
  id: string;
  path: string;
  content: string;
  language: string;
  status: 'draft' | 'saved' | 'applied';
  diff?: string;
}
```

**Key Actions:**
- `addMessage()` - Add new message
- `updateMessage()` - Update existing message
- `startStreaming()` / `endStreaming()` - Control streaming
- `appendStreamContent()` - Buffer streaming tokens
- `updateMessagePlan()` - Update plan in message
- `addMessageTask()` / `updateTaskStatus()` - Task tracking
- `addToolExecution()` - Track tool execution
- `reset()` - Clear all state

**Persistence:** LocalStorage (`mira-chat-storage`)

---

### 4.2 useWebSocketStore

**Purpose:** WebSocket connection and messaging

**Location:** `/frontend/src/stores/useWebSocketStore.ts`

```typescript
interface WebSocketStoreState {
  socket: WebSocket | null;
  connectionState: 'connecting' | 'connected' | 'reconnecting' | 'disconnected' | 'error';
  reconnectAttempts: number;
  maxReconnectAttempts: number;
  reconnectDelay: number;
  lastMessage: WebSocketMessage | null;
  messageQueue: WebSocketMessage[];
  listeners: Map<string, Subscriber>;
  isConnected: boolean;
}
```

**Key Actions:**
- `connect()` - Establish WebSocket to `ws://localhost:3001/ws`
- `disconnect()` - Close connection
- `send()` - Queue or send message
- `subscribe()` - Register listener
- `scheduleReconnect()` - Exponential backoff (up to 30s)
- `processMessageQueue()` - Send queued messages

**Auto-connect:** Connects 100ms after store creation

---

### 4.3 useAppState

**Purpose:** Application UI state, projects, artifacts

**Location:** `/frontend/src/stores/useAppState.ts`

```typescript
interface AppState {
  // UI
  showArtifacts: boolean;
  showFileExplorer: boolean;
  quickOpenVisible: boolean;

  // Projects
  currentProject: Project | null;
  projects: Project[];

  // Git
  modifiedFiles: string[];
  currentBranch: string;

  // Artifacts
  artifacts: Artifact[];
  activeArtifactId: string | null;
  appliedFiles: Set<string>;

  // Connection
  isReconnecting: boolean;
  reconnectAttempts: number;
  connectionStatus: string;
  connectionError: string | null;

  // Rate limiting
  canSendMessage: boolean;
  rateLimitUntil: number | null;

  // Notifications
  toasts: Toast[];

  // System access
  systemAccessMode: 'project' | 'home' | 'system';
}
```

**Key Actions:**
- `addArtifact()` / `updateArtifact()` / `removeArtifact()`
- `markArtifactApplied()`
- `setCurrentProject()` / `setProjects()` / `addProject()`
- `addToast()` / `removeToast()` (auto-dismiss)
- `addModifiedFile()` / `removeModifiedFile()`
- `setConnectionError()` / `setReconnecting()`
- `setRateLimitUntil()`

**Validation:**
- Artifact paths: Rejects `..` and system paths (`/etc`, `/usr`, `/var`)
- Requires `id` and valid `path`

**Persistence:** LocalStorage (`mira-app-state`)

**Convenience Hooks:**
- `useProjectState()` - currentProject, projects, modifiedFiles
- `useArtifactState()` - artifacts, activeArtifact, appliedFiles

---

### 4.4 useAuthStore

**Purpose:** Authentication and user state

**Location:** `/frontend/src/stores/useAuthStore.ts`

```typescript
interface AuthState {
  user: User | null;
  token: string | null;
  isAuthenticated: boolean;
}

interface User {
  id: string;
  username: string;
  displayName: string;
  email?: string;
  theme_preference?: string;
}
```

**Key Actions:**
- `login(username, password)` - POST `/api/auth/login`
- `register(username, password, email, displayName)` - POST `/api/auth/register`
- `verifyToken()` - POST `/api/auth/verify`
- `changePassword(current, new)` - POST `/api/auth/change-password`
- `logout()` - Clear state

**Selector Hooks:**
- `useCurrentUser()` - Get current user
- `useIsAuthenticated()` - Get auth status
- `useToken()` - Get JWT token

---

### 4.5 useCodeIntelligenceStore

**Purpose:** Code intelligence features (budget, search, suggestions)

**Location:** `/frontend/src/stores/useCodeIntelligenceStore.ts`

```typescript
interface CodeIntelligenceState {
  // Budget
  budget: BudgetStatus | null;
  isBudgetLoading: boolean;
  budgetError: string | null;

  // Code search
  codeSearch: {
    query: string;
    results: CodeSearchResult[];
    isSearching: boolean;
    error: string | null;
    lastSearchTime: number;
  };

  // Co-change suggestions
  coChangeSuggestions: CoChangeSuggestion[];
  isLoadingCoChange: boolean;
  currentFile: string | null;

  // Historical fixes
  historicalFixes: HistoricalFix[];
  isLoadingFixes: boolean;

  // Expertise
  expertise: AuthorExpertise[];
  isLoadingExpertise: boolean;

  // Build errors
  buildErrors: BuildError[];
  isLoadingBuildErrors: boolean;

  // Panel
  isPanelVisible: boolean;
  activeTab: 'budget' | 'search' | 'cochange' | 'builds' | 'tools' | 'permissions';
}

interface BudgetStatus {
  daily_limit_usd: number;
  daily_spent_usd: number;
  daily_remaining_usd: number;
  daily_usage_percent: number;
  monthly_limit_usd: number;
  monthly_spent_usd: number;
  monthly_remaining_usd: number;
  monthly_usage_percent: number;
}

interface CoChangeSuggestion {
  filePath: string;
  confidence: number;
  reason: string;
  coChangeCount: number;
}
```

**Key Actions:**
- `setBudget()` / `refreshBudget()`
- `setSearchQuery()` / `setSearchResults()` / `clearSearch()`
- `togglePanel()` / `setActiveTab()`

**Convenience Hooks:**
- `useBudgetStatus()` - Current budget
- `useCodeSearch()` - Search state
- `useCoChangeSuggestions()` - Co-change data
- `useBuildErrors()` - Build errors

---

### 4.6 useAgentStore

**Purpose:** Background agent (Codex) management

**Location:** `/frontend/src/stores/useAgentStore.ts`

```typescript
interface AgentStoreState {
  agents: BackgroundAgent[];
  isPanelVisible: boolean;
  loading: boolean;
  selectedAgentId: string | null;
}

interface BackgroundAgent {
  id: string;
  task: string;
  status: 'running' | 'completed' | 'failed' | 'cancelled';
  started_at: number;
  completed_at?: number;
  tokens_used: number;
  cost_usd: number;
  compaction_count: number;
  progress_percent?: number;
  current_activity?: string;
  completion_summary?: string;
}
```

**Key Actions:**
- `addAgent()` / `updateAgent()` / `removeAgent()`
- `togglePanel()` / `setPanelVisible()`
- `selectAgent()`

**Helper Hooks:**
- `useRunningAgentsCount()` - Badge count

---

### 4.7 useActivityStore

**Purpose:** Operation activity tracking

**Location:** `/frontend/src/stores/useActivityStore.ts`

```typescript
interface ActivityStoreState {
  isPanelVisible: boolean;
  panelWidth: number;  // 300-800px
  currentOperationId: string | null;
  currentMessageId: string | null;
}
```

**Key Actions:**
- `togglePanel()` / `showPanel()` / `hidePanel()`
- `setCurrentOperation(operationId, messageId)`
- `clearCurrentOperation()`
- `setPanelWidth(width)` - Clamped 300-800px

**Data Accessors:**
- `getCurrentPlan()` - Get plan from message
- `getCurrentTasks()` - Get tasks for operation
- `getCurrentToolExecutions()` - Get tool runs
- `getAllActivity()` - Combined list

---

### 4.8 useSudoStore

**Purpose:** Sudo approval and permissions

**Location:** `/frontend/src/stores/useSudoStore.ts`

```typescript
interface SudoStoreState {
  pendingApprovals: SudoApprovalRequest[];
  permissions: SudoPermission[];
  blocklist: SudoBlocklistEntry[];
  loading: boolean;
}

interface SudoApprovalRequest {
  id: string;
  command: string;
  reason: string;
  expiresAt: number;
  status: 'pending' | 'approved' | 'denied';
}

interface SudoPermission {
  name: string;
  command_pattern?: string;
  command_prefix?: string;
  command_exact?: string;
  requires_approval: boolean;
  enabled: boolean;
  use_count: number;
}
```

**Key Actions:**
- `approveRequest()` / `denyRequest()` - WebSocket commands
- `fetchPermissions()` / `addPermission()` / `togglePermission()`
- `fetchBlocklist()` / `addBlocklistEntry()` / `toggleBlocklistEntry()`

**Selector Hooks:**
- `usePendingApprovals()` - Memoized pending
- `useSudoPermissions()` - Current permissions
- `useSudoBlocklist()` - Current blocklist

---

### 4.9 useUsageStore

**Purpose:** LLM usage and pricing tracking

**Location:** `/frontend/src/stores/useUsageStore.ts`

```typescript
interface UsageStoreState {
  currentUsage: UsageInfo | null;
  sessionTotalCost: number;
  sessionTotalTokensInput: number;
  sessionTotalTokensOutput: number;
  cacheHits: number;
  cacheMisses: number;
  currentWarning: ContextWarning | null;
  warningDismissed: boolean;
  thinkingStatus: ThinkingStatus | null;
}

interface UsageInfo {
  operationId: string;
  tokensInput: number;
  tokensOutput: number;
  pricingTier: string;
  costUsd: number;
  fromCache: boolean;
}

interface ThinkingStatus {
  status: 'gathering_context' | 'thinking' | 'executing_tool' | 'idle';
  message: string;
  tokens?: number;
}
```

**Key Actions:**
- `updateUsage()` - Accumulate session totals
- `setWarning()` - Set context warning
- `setThinkingStatus()` / `clearThinkingStatus()`
- `resetSession()` - Clear tracking

---

### 4.10 useThemeStore

**Purpose:** Light/dark theme preference

**Location:** `/frontend/src/stores/useThemeStore.ts`

```typescript
interface ThemeState {
  theme: 'light' | 'dark';
}
```

**Key Actions:**
- `toggleTheme()` - Switch theme
- `initializeFromUser()` - Load from user preferences

---

### 4.11 useReviewStore

**Purpose:** Code review panel state

**Location:** `/frontend/src/stores/useReviewStore.ts`

```typescript
interface ReviewState {
  isReviewMode: boolean;
  isPanelVisible: boolean;
  loading: boolean;
  diff: string | null;
  reviewTarget: 'uncommitted' | 'staged' | 'branch' | 'commit';
  baseBranch: string;
  commitHash: string;
  reviewResult: string | null;
  reviewing: boolean;
  additions: number;
  deletions: number;
  filesChanged: string[];
}
```

**Key Actions:**
- `togglePanel()` / `setPanelVisible()`
- `setDiff()` - Parses stats automatically
- `setReviewTarget()` / `setBaseBranch()` / `setCommitHash()`
- `setReviewResult()` / `setReviewing()`

---

## 5. Custom React Hooks

### 5.1 useWebSocketMessageHandler

**Purpose:** Global WebSocket message dispatcher

**Location:** `/frontend/src/hooks/useWebSocketMessageHandler.ts`

**Handles:**
- **Operation events**: started, streaming, completed, failed, status_changed
- **Planning mode**: plan_generated, task_created/started/completed/failed
- **Tool execution**: operation.tool_executed
- **Agent events**: agent_spawned, agent_progress, agent_completed
- **Codex events**: codex.spawned, codex.progress, codex.completed
- **Project/file**: projects, project_list, local_directory_attached
- **Git status**: git_status with modified files
- **Code intelligence**: budget_status, semantic_search_results, cochange_suggestions
- **Sudo**: sudo_pending_approvals, sudo_permissions, sudo_blocklist
- **Usage**: operation.usage_info, operation.context_warning, operation.thinking

---

### 5.2 useMessageHandler

**Purpose:** Chat response message handler

**Location:** `/frontend/src/hooks/useMessageHandler.ts`

**Message Types:**
- `status` - Thinking indicator
- `stream` - Token deltas
- `chat_complete` - Finalize message
- `response` - Legacy format
- `operation.tool_executed` - Tool execution

**Features:**
- Extracts artifacts from responses
- Creates toast notifications for file operations
- Appends tool executions to streaming messages

---

### 5.3 useChatMessaging

**Purpose:** Chat message sending

**Location:** `/frontend/src/hooks/useChatMessaging.ts`

**Exports:**
- `handleSend(content)` - Send chat message
- `addSystemMessage(content)` - Add system message
- `addProjectContextMessage(name)` - Notify project change
- `addFileContextMessage(name)` - Notify file change

**Context Attached:**
- `project_id`, `system_access_mode`
- `session_id` (from auth user)
- `file_path`, `file_content`, `language`
- `has_repository`, `current_branch`, `modified_files_count`

---

### 5.4 useChatPersistence

**Purpose:** Load and sync chat history

**Location:** `/frontend/src/hooks/useChatPersistence.ts`

**Features:**
- Load messages from backend on connection
- Persist messages to localStorage
- Sync when connectionState changes

---

### 5.5 useArtifacts

**Purpose:** Artifact management hook

**Location:** `/frontend/src/hooks/useArtifacts.ts`

**Exports:**
- Artifact CRUD operations
- Status tracking (draft/saved/applied)
- Filtering and sorting

---

### 5.6 useProjectOperations

**Purpose:** Project/repository management

**Location:** `/frontend/src/hooks/useProjectOperations.ts`

**Features:**
- Create projects
- Attach directories
- Manage repositories
- List/delete projects

---

### 5.7 useGitOperations

**Purpose:** Git operations

**Location:** `/frontend/src/hooks/useGitOperations.ts`

**Features:**
- Get git status
- Fetch diffs
- Get branch info
- View commit history

---

### 5.8 useSessionOperations

**Purpose:** Session management

**Location:** `/frontend/src/hooks/useSessionOperations.ts`

**Features:**
- Create sessions
- Resume sessions
- Fork sessions
- List/delete sessions

---

### 5.9 useErrorHandler

**Purpose:** WebSocket error handling

**Location:** `/frontend/src/hooks/useErrorHandler.ts`

**Features:**
- Convert WS errors to chat messages
- Show error toasts
- Track connection errors

---

### 5.10 useConnectionTracking

**Purpose:** Sync WebSocket state to AppState

**Location:** `/frontend/src/hooks/useConnectionTracking.ts`

---

### 5.11 useCodeIntelligenceHandler

**Purpose:** Code intelligence WebSocket handler

**Location:** `/frontend/src/hooks/useCodeIntelligenceHandler.ts`

**Handles:**
- Budget status updates
- Search results
- Co-change suggestions
- Historical fixes
- Expertise data

---

### 5.12 useArtifactFileContentWire

**Purpose:** File content → artifact bridge

**Location:** `/frontend/src/hooks/useArtifactFileContentWire.ts`

**Purpose:** Convert file_content events to artifacts

---

### 5.13 useToolResultArtifactBridge

**Purpose:** Tool results → artifacts

**Location:** `/frontend/src/hooks/useToolResultArtifactBridge.ts`

**Purpose:** Convert tool execution results to artifacts

---

## 6. Components

### 6.1 Layout Components

**Home.tsx** - Main application layout
- Orchestrates WebSocket connection
- Initializes all message handlers
- Arranges: Header, ChatArea, ArtifactPanel, ActivityPanel, IntelligencePanel, AgentsPanel, ReviewPanel

**Header.tsx** - Top navigation bar (height: 56px)
- Project selector (opens ProjectsView)
- Session manager (opens SessionsModal)
- User menu (change password)
- Panel toggles: Activity, Intelligence, Agents, Review
- Theme toggle (light/dark)
- Logout button

**ChatArea.tsx** - Chat container
- ConnectionBanner (if disconnected)
- MessageList (scrollable)
- SudoApprovalInline (if pending)
- ChatInput (message input)

---

### 6.2 Message Components

**MessageList.tsx** - Scrollable message history
- Uses react-virtuoso for virtualization
- Auto-scroll on new messages

**ChatMessage.tsx** - Individual message
- Role-based styling
- Markdown rendering
- Artifact preview links
- Task list display
- Tool execution summary

**ChatInput.tsx** - Message input
- Enter to send, Shift+Enter for newline
- Disabled when waiting/rate limited
- Rate limit countdown

---

### 6.3 Artifact Components

**ArtifactPanel.tsx** - Side panel
- Monaco editor for viewing/editing
- Apply button
- Language-specific highlighting
- Diff view support

**MonacoEditor.tsx** - Embedded code editor
- Language detection
- Theme support
- Read-only mode

**CodeBlock.tsx** - Inline code snippet
- Syntax highlighting
- Copy button
- Language tag

**UnifiedDiffView.tsx** - Git-style diff viewer
- Added/removed line highlighting
- Line numbers

---

### 6.4 Activity & Operations Components

**ActivityPanel.tsx** - Right side panel
- Plan display
- Task list with status
- Tool executions summary
- Dynamic width (300-800px)

**ActivitySections/**
- `PlanDisplay.tsx` - Operation plan text
- `TasksSection.tsx` - Task list
- `ToolExecutionsSection.tsx` - Tool results
- `ReasoningSection.tsx` - Extended reasoning

**ThinkingIndicator.tsx** - Animated loading
- Shows LLM thinking status
- Current activity display

**TaskTracker.tsx** - Task item
- Status badge
- Description
- Timestamp
- Error details

---

### 6.5 Intelligence & Budget Components

**IntelligencePanel.tsx** - Right side panel with tabs:
- Budget tracker
- Semantic search
- Co-change suggestions
- Build errors
- Sudo permissions

**BudgetTracker.tsx** - Budget display
- Daily/monthly progress bars
- Critical/low warnings
- Cache hit rate

**SemanticSearch.tsx** - Code search
- Query input
- Results table (filePath, snippet, score)

**CoChangeSuggestions.tsx** - Files changed together
- Suggestion list with confidence
- Last modified date

**BuildErrorsPanel.tsx** - Build errors
- Error list with severity
- Suggested fixes

---

### 6.6 Sudo & Permissions Components

**SudoApprovalInline.tsx** - Approval widget
- Command display
- Approve/Deny buttons
- Expiration countdown

**PermissionsPanel.tsx** - Manage permissions
- Permission list with toggle
- Add permission form
- Blocklist management

---

### 6.7 Project & File Components

**ProjectsView.tsx** - Project management
- List existing projects
- Create project form
- Delete project
- Attach directory/repository

**FileBrowser.tsx** - File tree navigator
- Hierarchical display
- File click to view
- Expandable folders
- File icons by type

**CreateProjectModal.tsx** - Project creation
**ProjectSettingsModal.tsx** - Edit settings
**OpenDirectoryModal.tsx** - Attach directory
**CodebaseAttachModal.tsx** - Attach repository

---

### 6.8 Session Components

**SessionsModal.tsx** - Session management
- List active sessions
- Resume/fork/delete
- Session timestamps

---

### 6.9 Document Components

**DocumentsView.tsx** - Main documents interface
**DocumentUpload.tsx** - File upload with drag-and-drop
**DocumentSearch.tsx** - Semantic document search
**DocumentList.tsx** - Browse uploaded documents
**DocumentsModal.tsx** - Modal wrapper

---

### 6.10 Other Components

**ConnectionBanner.tsx** - Connection status indicator
**ToastContainer.tsx** - Toast notification manager
**BackgroundAgentsPanel.tsx** - Background task monitor
**ReviewPanel.tsx** - Code review modal
**ToolsDashboard.tsx** - Available tools display
**UsageIndicator.tsx** - Usage/cost display
**PrivateRoute.tsx** - Route protection
**ChangePasswordModal.tsx** - Password change

---

## 7. Type Definitions

**Location:** `/frontend/src/types/index.ts`

```typescript
// Sessions
interface Session {
  id: string;
  user_id?: string;
  name?: string;
  project_path?: string;
  last_message_preview?: string;
  message_count: number;
  created_at: number;  // Unix timestamp
  last_active: number;
}

// Projects
interface Project {
  id: string;
  name: string;
  description?: string;
  tags?: string[];
  owner?: string;
  has_repository?: boolean;
  has_codebase?: boolean;
  repository_url?: string;
  import_status?: string;
  last_sync_at?: string | null;
  created_at: string;  // RFC3339
  updated_at: string;
}

// Documents
interface DocumentMetadata {
  id: string;
  file_name: string;
  file_type: string;
  size_bytes: number;
  chunk_count: number;
  word_count?: number;
  created_at: string;
  project_id?: string;
}

interface DocumentSearchResult {
  chunk_id: string;
  chunk_index: number;
  document_id: string;
  file_name: string;
  content: string;
  score: number;
  page_number?: number;
}

// File System
interface FileNode {
  name: string;
  path: string;
  type: 'file' | 'directory';
  children?: FileNode[];
  size?: number;
  modified?: string;
}

// Tool Results
interface ToolResult {
  id: string;
  type: 'web_search' | 'code_execution' | 'file_operation' |
        'git_operation' | 'code_analysis' | 'code_search' |
        'repository_stats' | 'complexity_analysis';
  status: 'success' | 'error' | 'pending';
  data: any;
  timestamp: number;
}
```

---

## 8. WebSocket Communication

### 8.1 Connection Details

- **URL:** `ws://localhost:3001/ws`
- **Auth:** Optional token in query parameter
- **Port:** 3001 (NOT 8080)

### 8.2 Message Protocol

```typescript
interface WebSocketMessage {
  type: string;
  [key: string]: any;
}
```

### 8.3 Client → Server Messages

```json
// Chat message
{
  "type": "chat",
  "content": "User message",
  "project_id": "proj-123",
  "session_id": "sess-456",
  "system_access_mode": "project",
  "metadata": { "file_path": "src/main.rs" }
}

// Project operations
{
  "type": "project_command",
  "method": "create",
  "params": { "name": "my-project" }
}

// Git operations
{
  "type": "git_command",
  "method": "git.diff",
  "params": { "project_id": "proj-123", "target": "uncommitted" }
}

// Sudo approval
{
  "type": "sudo_approval",
  "approval_id": "req-789",
  "approved": true
}
```

### 8.4 Server → Client Messages

```json
// Data envelope
{
  "type": "data",
  "data": {
    "type": "operation.started",
    "operation_id": "op-123"
  }
}

// Stream token
{
  "type": "stream",
  "content": "token text",
  "operation_id": "op-123"
}

// Operation event (top-level)
{
  "type": "operation.tool_executed",
  "operation_id": "op-123",
  "tool_name": "read_file",
  "success": true
}
```

### 8.5 Subscription Pattern

```typescript
const unsubscribe = subscribe(
  'listener-id',
  (message) => { /* handle */ },
  ['data', 'operation.started']  // Optional filter
);
```

---

## 9. Routes & Pages

| Path | Component | Protection | Purpose |
|------|-----------|------------|---------|
| `/login` | `Login.tsx` | Public | Authentication |
| `/` | `Home.tsx` | Private | Main application |
| `*` | Redirect to `/` | - | 404 handling |

**Private Route Logic:**
- Checks `useAuthStore().isAuthenticated`
- Redirects to `/login` if not authenticated
- Verifies token on app load

---

## 10. State Flow Diagram

```
User Input (ChatInput)
         ↓
   useChatMessaging.handleSend()
         ↓
   WebSocketStore.send()
         ↓
   Backend (ws://localhost:3001)
         ↓
   WebSocketStore.handleMessage()
         ↓
    ┌────┴─────┬──────────┬─────────────┐
    ↓          ↓          ↓             ↓
Data Type   Status    Error         Sudo Approval
    ↓
useWebSocketMessageHandler
    ↓
Dispatch to stores:
├── ChatStore (messages, tasks, plan)
├── AppState (artifacts, projects)
├── CodeIntelligenceStore (budget, errors)
├── ActivityStore (operation tracking)
├── UsageStore (pricing, warnings)
└── SudoStore (approvals)
    ↓
Components re-render via Zustand selectors
```

---

## 11. Data Persistence

| Store | Method | Key | What's Saved |
|-------|--------|-----|--------------|
| useChatStore | localStorage | `mira-chat-storage` | messages, currentSessionId |
| useAppState | localStorage | `mira-app-state` | currentProject, projects, artifacts, appliedFiles |
| useAuthStore | localStorage | `mira-auth-storage` | user, token, isAuthenticated |
| Others | Memory only | - | - |

---

## 12. Configuration

### 12.1 App Configuration

**Location:** `/frontend/src/config/app.ts`

```typescript
const APP_CONFIG = {
  WS_URL: import.meta.env.VITE_WS_URL || 'ws://localhost:3001/ws',
  API_URL: import.meta.env.VITE_API_URL || 'http://localhost:3001',
  ENABLE_AUTH: true,
  ENABLE_MULTI_USER: true
};
```

### 12.2 Vite Configuration

**Location:** `/frontend/vite.config.js`

```javascript
{
  plugins: [react()],
  server: {
    proxy: {
      '/api': 'http://localhost:3001',
      '/ws': 'ws://localhost:3001'
    }
  }
}
```

### 12.3 Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `VITE_WS_URL` | Derived from host | WebSocket endpoint |
| `VITE_API_URL` | http://localhost:3001 | REST API endpoint |
| `VITE_ENABLE_AUTH` | true | Enable authentication |
| `VITE_ENABLE_MULTI_USER` | true | Multi-user support |

---

## 13. Component Dependency Map

```
App.tsx
├── Login.tsx (route)
└── Home.tsx (route)
    ├── Header.tsx
    │   ├── ProjectsView.tsx (modal)
    │   ├── SessionsModal.tsx (modal)
    │   └── ChangePasswordModal.tsx (modal)
    │
    ├── ChatArea.tsx
    │   ├── ConnectionBanner.tsx
    │   ├── MessageList.tsx
    │   │   └── ChatMessage.tsx (repeated)
    │   │       ├── CodeBlock.tsx
    │   │       └── TaskTracker.tsx
    │   ├── SudoApprovalInline.tsx (repeated)
    │   └── ChatInput.tsx
    │
    ├── ArtifactPanel.tsx
    │   └── MonacoEditor.tsx
    │       └── UnifiedDiffView.tsx
    │
    ├── ActivityPanel.tsx
    │   ├── PlanDisplay.tsx
    │   ├── TasksSection.tsx
    │   ├── ToolExecutionsSection.tsx
    │   └── ReasoningSection.tsx
    │
    ├── IntelligencePanel.tsx
    │   ├── BudgetTracker.tsx
    │   ├── SemanticSearch.tsx
    │   ├── CoChangeSuggestions.tsx
    │   ├── BuildErrorsPanel.tsx
    │   └── PermissionsPanel.tsx
    │
    ├── BackgroundAgentsPanel.tsx
    ├── ReviewPanel.tsx (modal)
    └── ToastContainer.tsx
```

---

## 14. Testing Infrastructure

### 14.1 Test Framework

- **Vitest** - Unit testing
- **React Testing Library** - Component testing

### 14.2 Test Locations

```
frontend/src/
├── __tests__/           # Integration tests
├── stores/__tests__/    # Store tests
├── hooks/__tests__/     # Hook tests
└── components/__tests__/ # Component tests
```

### 14.3 Key Test Suites

- `appState.test.ts` - AppState store
- `chatStore.test.ts` - ChatStore
- `websocketStore.test.ts` - WebSocket
- `authStore.test.ts` - Authentication
- Component-specific tests

### 14.4 Running Tests

```bash
npm run test              # Run once
npm run test:watch        # Watch mode
npm run test:ui           # UI mode
npm run test:coverage     # With coverage
```

---

## 15. Theme Support

### 15.1 Theme Store

```typescript
interface ThemeState {
  theme: 'light' | 'dark';
  toggleTheme: () => void;
  initializeFromUser: () => void;
}
```

### 15.2 CSS Classes

- **Light:** `text-gray-900 bg-white`
- **Dark:** `dark:text-slate-100 dark:bg-slate-900`

Applied via TailwindCSS `dark:` prefix.

---

## Appendix: Quick Reference

### Running the Frontend

```bash
# Development
npm run dev

# Build
npm run build

# Type checking
npm run type-check

# Preview production build
npm run preview
```

### WebSocket Connection

```typescript
// Connect
const { connect, send, subscribe } = useWebSocketStore();
connect();

// Send message
send({ type: 'chat', content: 'Hello' });

// Subscribe to messages
const unsubscribe = subscribe('id', (msg) => { ... }, ['data']);
```

### Store Usage

```typescript
// Get state
const { messages, addMessage } = useChatStore();
const { currentProject } = useAppState();
const { isAuthenticated, login } = useAuthStore();

// Convenience hooks
const user = useCurrentUser();
const budget = useBudgetStatus();
const { artifacts, activeArtifact } = useArtifactState();
```
