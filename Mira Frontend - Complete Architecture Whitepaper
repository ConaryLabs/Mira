# Mira Frontend - Complete Architecture Whitepaper

**Version:** 0.1.0  
**Tech Stack:** React + TypeScript + Vite + Tailwind CSS + Zustand + Monaco Editor  
**Communication:** WebSocket (real-time bidirectional)  
**Date:** October 2025

---

## Table of Contents

1. [Overview](#overview)
2. [Project Structure](#project-structure)
3. [Configuration Files](#configuration-files)
4. [Core Architecture](#core-architecture)
5. [State Management (Zustand Stores)](#state-management-zustand-stores)
6. [Hooks System](#hooks-system)
7. [Components](#components)
8. [Services](#services)
9. [Types & Interfaces](#types--interfaces)
10. [WebSocket Protocol](#websocket-protocol)
11. [Data Flow](#data-flow)
12. [Key Features](#key-features)
13. [Performance Optimizations](#performance-optimizations)
14. [Development Guidelines](#development-guidelines)

---

## Overview

Mira Frontend is a modern, real-time chat interface for interacting with an AI assistant that can manage projects, execute Git operations, analyze code, and process documents. The frontend is built with a focus on **performance**, **real-time updates**, and **developer experience**.

### Key Characteristics

- **Real-time WebSocket communication** with backend
- **Persistent state** using Zustand + localStorage
- **Optimized rendering** with React.memo and selective subscriptions
- **Code editing** with Monaco Editor (VSCode's editor)
- **Artifact system** for code/file management
- **Project-based workflow** with Git integration
- **Document upload/search** with semantic search capabilities
- **Virtualized lists** for performance with large message histories

---

## Project Structure

```
mira-frontend/
├── src/
│   ├── components/          # React components
│   │   ├── documents/       # Document management components
│   │   │   ├── DocumentsView.tsx
│   │   │   ├── DocumentSearch.tsx
│   │   │   ├── DocumentUpload.tsx
│   │   │   ├── DocumentList.tsx
│   │   │   └── index.ts
│   │   ├── modals/          # Modal components
│   │   │   ├── FileSearchModal.tsx
│   │   │   ├── ImageGenerationModal.tsx
│   │   │   └── index.ts
│   │   ├── App.tsx          # Root component
│   │   ├── ArtifactPanel.tsx           # Code/file viewer with Monaco
│   │   ├── ArtifactToggle.tsx          # Toggle button for artifact panel
│   │   ├── ChatArea.tsx                # Main chat container
│   │   ├── ChatInput.tsx               # Message input field
│   │   ├── ChatMessage.tsx             # Individual message display
│   │   ├── CodeBlock.tsx               # Syntax-highlighted code blocks
│   │   ├── CommitPushButton.tsx        # Git commit/push UI
│   │   ├── ConnectionBanner.tsx        # WebSocket status indicator
│   │   ├── FileBrowser.tsx             # File tree navigation
│   │   ├── GitSyncButton.tsx           # Git sync operations
│   │   ├── Header.tsx                  # Top navigation bar
│   │   ├── MessageBubble.tsx           # Chat message bubble (memoized)
│   │   ├── MessageList.tsx             # Virtualized message list
│   │   ├── MonacoEditor.tsx            # Monaco editor wrapper
│   │   ├── ProjectsView.tsx            # Project management UI
│   │   ├── QuickFileOpen.tsx           # Cmd+P file picker
│   │   └── ThinkingIndicator.tsx       # Loading/thinking animation
│   │
│   ├── hooks/               # Custom React hooks
│   │   ├── useArtifacts.ts             # Artifact management
│   │   ├── useChatMessaging.ts         # Send messages with context
│   │   ├── useChatPersistence.ts       # Load/restore chat history
│   │   ├── useMessageHandler.ts        # Process incoming messages
│   │   └── useWebSocketMessageHandler.ts # WebSocket message routing
│   │
│   ├── stores/              # Zustand state stores
│   │   ├── useAppState.ts              # Global app state (projects, artifacts)
│   │   ├── useAuthStore.ts             # Authentication (hardcoded for now)
│   │   ├── useChatStore.ts             # Chat messages and streaming
│   │   ├── useUIStore.ts               # Transient UI state (input, modals)
│   │   └── useWebSocketStore.ts        # WebSocket connection & messaging
│   │
│   ├── services/            # External service integrations
│   │   └── BackendCommands.ts          # Backend command API wrapper
│   │
│   ├── types/               # TypeScript type definitions
│   │   └── index.ts                    # All shared types
│   │
│   ├── main.tsx             # React entry point
│   ├── App.tsx              # Root application component
│   ├── App.css              # Global styles
│   ├── index.css            # Tailwind imports
│   └── vite-env.d.ts        # Vite environment types
│
├── index.html               # HTML entry point
├── package.json             # Dependencies and scripts
├── tsconfig.json            # TypeScript configuration
├── vite.config.js           # Vite build configuration
├── tailwind.config.js       # Tailwind CSS configuration
├── postcss.config.js        # PostCSS configuration
├── .gitignore               # Git ignore rules
├── LICENSE                  # MIT License
└── README.md                # Project documentation
```

---

## Configuration Files

### `package.json`

**Dependencies:**
- **React 18.2** - UI framework
- **Zustand 5.0** - State management
- **@monaco-editor/react 4.7** - Code editor component
- **react-markdown 10.1** - Markdown rendering
- **react-syntax-highlighter 15.6** - Code syntax highlighting
- **react-virtuoso 4.14** - Virtualized list rendering
- **lucide-react 0.365** - Icon library
- **clsx 2.1** - Conditional classnames
- **ws 8.18** - WebSocket client

**Dev Dependencies:**
- **Vite 7.1** - Build tool
- **TypeScript 5.2** - Type safety
- **Tailwind CSS 3.4** - Utility-first CSS
- **ESLint** - Code linting

**Scripts:**
```json
{
  "dev": "vite",                    // Development server
  "build": "tsc && vite build",     // Production build
  "preview": "vite preview",        // Preview production build
  "type-check": "tsc --noEmit"      // Type checking only
}
```

---

### `vite.config.js`

```javascript
export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      '/api': {
        target: 'http://localhost:3001',  // Backend REST API
        changeOrigin: true,
      },
      '/ws': {
        target: 'ws://localhost:3001',    // WebSocket endpoint
        ws: true,
        changeOrigin: true,
      },
    },
  },
});
```

- **Dev server proxies** API and WebSocket requests to backend
- **No environment-specific config** - uses single backend port (3001)

---

### `tailwind.config.js`

Custom animations and theme extensions:

- **Custom animations:** fade-in, slide-in, shimmer, typing-dots, pulse-slow
- **Dark mode:** class-based (always dark in practice)
- **No custom utilities** - uses inline styles for dynamic delays

---

### `tsconfig.json`

- **Target:** ES2020
- **Module:** ESNext with bundler resolution
- **JSX:** react-jsx (modern transform)
- **Strict mode:** enabled
- **Types:** includes node types for process.env

---

## Core Architecture

### Application Entry Points

**`index.html`**
- Minimal HTML shell
- Single root div: `<div id="root"></div>`
- Loads `src/main.tsx` as module

**`src/main.tsx`**
```typescript
ReactDOM.createRoot(document.getElementById('root')!).render(
  <App />
);
```
- **No StrictMode** - intentionally removed to avoid double-render issues with WebSocket connections

**`src/App.tsx`**
- Root component that initializes:
  - WebSocket connection on mount
  - Message handlers (WebSocket, chat persistence)
  - Tab navigation (Chat vs Projects)
  - Quick file open (Cmd+P)
  - Artifact panel visibility

```typescript
// Core responsibilities:
1. Connect WebSocket on mount
2. Subscribe to WebSocket messages
3. Load chat history from backend
4. Render Header, ChatArea, ArtifactPanel, ProjectsView
5. Handle keyboard shortcuts (Cmd+P)
```

---

## State Management (Zustand Stores)

Mira uses **Zustand** for state management with **localStorage persistence** for critical data.

### Store Architecture

```
┌─────────────────────────────────────────────────┐
│              State Stores                        │
├─────────────────────────────────────────────────┤
│ useAppState        - Projects, Artifacts, Git   │
│ useChatStore       - Messages, Streaming        │
│ useWebSocketStore  - Connection, Messaging      │
│ useUIStore         - Input, Modals, Tabs        │
│ useAuthStore       - User (hardcoded)           │
└─────────────────────────────────────────────────┘
```

---

### `useAppState.ts` - Global Application State

**Persisted to localStorage:** `mira-app-state`

**State:**
```typescript
interface AppState {
  // UI State
  showArtifacts: boolean;
  showFileExplorer: boolean;
  quickOpenVisible: boolean;
  
  // Project State
  currentProject: Project | null;
  projects: Project[];
  
  // Git State
  modifiedFiles: string[];
  currentBranch: string;
  gitStatus: any;
  
  // Artifacts (code/file viewer)
  artifacts: Artifact[];
  activeArtifactId: string | null;
  appliedFiles: Set<string>;  // Track which artifacts have been applied to disk
  
  // Code Intelligence
  codeAnalysis: any;
  complexityHotspots: any[];
  
  // Memory & Context
  relevantMemories: any[];
  recentTopics: string[];
}
```

**Key Actions:**
- `setCurrentProject()` - Switch active project
- `addArtifact()` - Add code/file to artifact viewer
- `setActiveArtifact()` - Switch active tab in artifact panel
- `markArtifactApplied()` - Track which files have been written to disk
- `setProjects()` - Update project list with reference stability fix

**Persistence Strategy:**
- **Persisted:** projects, currentProject, artifacts, activeArtifactId, appliedFiles
- **Not persisted:** UI state, git status, temporary data

**Custom serialization** for Set types (appliedFiles)

---

### `useChatStore.ts` - Chat Messages & Streaming

**Persisted to localStorage:** `mira-chat-storage`

**State:**
```typescript
interface ChatStore {
  messages: ChatMessage[];           // Full message history
  currentSessionId: string;          // "peter-eternal" (eternal session)
  isWaitingForResponse: boolean;     // Show thinking indicator
  isStreaming: boolean;              // Streaming in progress
  streamingContent: string;          // Accumulated streaming content
  streamingMessageId: string | null; // ID of streaming message
}
```

**Message Structure:**
```typescript
interface ChatMessage {
  id: string;
  role: 'user' | 'assistant' | 'system';
  content: string;
  timestamp: number;
  artifacts?: Artifact[];  // Code/files attached to message
  metadata?: {
    session_id?: string;
    project_id?: string;
    file_path?: string;
    [key: string]: any;
  };
}
```

**Artifact Structure (Unified):**
```typescript
interface Artifact {
  id: string;
  title?: string;       // Display name (optional)
  path: string;         // File path - primary identifier
  content: string;      // File content
  language?: string;    // Syntax highlighting
  changeType?: 'primary' | 'import' | 'type' | 'cascade';
  timestamp?: number;
}
```

**Key Actions:**
- `addMessage()` - Add new message to history
- `setMessages()` - Replace entire message array (used for loading from backend)
- `startStreaming()` - Begin streaming response
- `appendStreamContent()` - Add chunk to streaming buffer
- `endStreaming()` - Finalize streaming message

**Persistence:**
- **Persisted:** messages, currentSessionId
- **Not persisted:** streaming state, waiting state

---

### `useWebSocketStore.ts` - WebSocket Connection

**NOT persisted** (runtime state only)

**State:**
```typescript
interface WebSocketStore {
  socket: WebSocket | null;
  connectionState: 'connecting' | 'connected' | 'disconnected' | 'error';
  reconnectAttempts: number;
  maxReconnectAttempts: number;
  reconnectDelay: number;
  lastMessage: WebSocketMessage | null;
  messageQueue: WebSocketMessage[];  // Messages queued while disconnected
  listeners: Map<string, Subscriber>; // Message subscribers with filters
}
```

**Subscriber System (CRITICAL):**
```typescript
interface Subscriber {
  callback: (message: WebSocketMessage) => void;
  messageTypes?: string[];  // Optional filter - only notify for specific types
}
```

**Message Type Filters:**
- If `messageTypes` is undefined → receives ALL messages
- If `messageTypes` is defined → only receives matching types

**Example Usage:**
```typescript
// Subscribe to specific message types
subscribe('chat-handler', handleMessage, ['response']);
subscribe('doc-upload', handleDocUpload, ['data', 'error']);
subscribe('global-handler', handleAll); // No filter = all messages
```

**Key Actions:**
- `connect()` - Establish WebSocket connection
- `disconnect()` - Close connection
- `send(message)` - Send message (queues if disconnected)
- `subscribe(id, callback, messageTypes?)` - Subscribe to messages with optional filter

**Auto-reconnect Strategy:**
- Exponential backoff: delay = min(baseDelay * 2^attempts, 30000ms)
- Max 10 attempts
- Processes queued messages on reconnect

**Connection Lifecycle:**
```typescript
1. Auto-connect on store initialization (100ms delay)
2. On connect: set state to 'connected', process message queue
3. On close: set state to 'disconnected', schedule reconnect
4. On error: set state to 'error'
```

---

### `useUIStore.ts` - Transient UI State

**NOT persisted** (ephemeral state)

**State:**
```typescript
interface UIState {
  inputContent: string;           // Chat input field content
  isWaitingForResponse: boolean;  // Duplicate of chatStore (legacy?)
  activeModal: string | null;     // Currently open modal ID
  activeTab: Tab;                 // 'chat' | 'projects'
}
```

**Optimized Selectors:**
```typescript
export const useInputContent = () => useUIStore(state => state.inputContent);
export const useActiveTab = () => useUIStore(state => state.activeTab);
```

These selectors prevent unnecessary re-renders when other UI state changes.

---

### `useAuthStore.ts` - Authentication

**Persisted to localStorage:** `mira-auth-storage`

**State:**
```typescript
interface AuthState {
  user: User | null;
  token: string | null;
  isAuthenticated: boolean;
}
```

**Current Implementation:**
- **Hardcoded to "peter-eternal"** - no real authentication yet
- Session ID: `"peter-eternal"` (matches backend's eternal session)
- Ready for future OAuth/JWT implementation

---

## Hooks System

### Custom Hooks Overview

```
┌──────────────────────────────────────────┐
│         Custom Hooks                      │
├──────────────────────────────────────────┤
│ useWebSocketMessageHandler               │
│   ↳ Routes data messages to appropriate  │
│     handlers (projects, files, git)      │
│                                           │
│ useMessageHandler                         │
│   ↳ Processes chat response messages     │
│   ↳ Extracts and stores artifacts        │
│                                           │
│ useChatMessaging                          │
│   ↳ Sends messages with full context     │
│   ↳ Attaches project, file, git info     │
│                                           │
│ useChatPersistence                        │
│   ↳ Loads chat history from backend      │
│   ↳ Converts memory to messages          │
│   ↳ Restores artifacts from history      │
│                                           │
│ useArtifacts                              │
│   ↳ Manages artifact panel state         │
│   ↳ Save/copy/update artifacts           │
└──────────────────────────────────────────┘
```

---

### `useWebSocketMessageHandler.ts`

**Purpose:** Routes incoming WebSocket messages to appropriate handlers

**Subscribed Message Types:** `['data', 'status', 'error']`

**Handles:**
- `project_list` - Update projects in state
- `project_created` - Refresh project list
- `local_directory_attached` - Refresh project list
- `git_status` - Update git state
- `file_tree` - File list for QuickFileOpen
- `file_content` - Create artifact from file
- `document_*` - Document processing events (delegated to components)

**Key Logic:**
```typescript
// Detect language from file extension
const detectLanguage = (filePath: string): string => {
  const ext = filePath.split('.').pop()?.toLowerCase();
  // Returns: 'rust', 'typescript', 'python', etc.
};

// Create artifact from file
if (data.type === 'file_content') {
  const newArtifact: Artifact = {
    id: generateId(),
    path: data.path,
    content: data.content,
    language: detectLanguage(data.path),
  };
  addArtifact(newArtifact);
}
```

---

### `useMessageHandler.ts`

**Purpose:** Process incoming chat response messages

**Subscribed Message Types:** `['response']`

**Handles:**
- Streaming responses (incremental content)
- Complete responses (full message)
- Artifact extraction and storage

**Key Logic:**
```typescript
function handleChatResponse(message: any) {
  // Handle streaming
  if (message.streaming) {
    appendStreamContent(message.content);
    return;
  }
  
  // Handle stream completion
  if (message.complete) {
    endStreaming();
    return;
  }
  
  // Handle full response
  const content = message.data?.content || message.content;
  const artifacts = message.data?.artifacts || message.artifacts || [];
  
  // Add message to chat
  addMessage({ role: 'assistant', content, artifacts, ... });
  
  // Process artifacts (add to artifact panel)
  if (artifacts.length > 0) {
    processArtifacts(artifacts);
  }
}
```

**Artifact Processing:**
```typescript
function processArtifacts(artifacts: any[]) {
  artifacts.forEach(artifact => {
    const cleanArtifact: Artifact = {
      id: artifact.id || generateId(),
      path: artifact.path || artifact.title || 'untitled',
      content: artifact.content,
      language: artifact.language || inferLanguage(artifact.path),
      changeType: artifact.change_type,
    };
    addArtifact(cleanArtifact);
  });
  setShowArtifacts(true);
}
```

---

### `useChatMessaging.ts`

**Purpose:** Send messages with full project/file context

**Key Function:**
```typescript
const handleSend = async (content: string) => {
  // Add user message immediately
  addMessage({ role: 'user', content, timestamp: Date.now() });
  
  // Set waiting state BEFORE sending
  setWaitingForResponse(true);
  
  // Build message with context
  const message = {
    type: 'chat',
    content,
    project_id: currentProject?.id || null,
    metadata: {
      session_id: 'peter-eternal',
      timestamp: Date.now(),
      
      // FILE CONTEXT
      file_path: activeArtifact?.path || null,
      file_content: activeArtifact?.content || null,
      language: detectLanguage(activeArtifact?.path),
      
      // PROJECT CONTEXT
      has_repository: currentProject?.has_repository || false,
      current_branch: currentBranch || 'main',
      modified_files_count: modifiedFiles.length,
    }
  };
  
  await send(message);
};
```

**Context Enrichment:**
- Includes active artifact (file being viewed)
- Includes project state (has repo, branch, modified files)
- Attaches session ID for memory continuity

---

### `useChatPersistence.ts`

**Purpose:** Load and restore chat history from backend

**Lifecycle:**
1. Wait for WebSocket connection
2. Request recent memories from backend
3. Convert memories to messages
4. Restore artifacts from memory
5. Open artifact panel if artifacts found

**Key Logic:**
```typescript
const loadChatHistory = async () => {
  await send({
    type: 'memory_command',
    method: 'memory.get_recent',
    params: {
      session_id: 'peter-eternal',
      count: 100
    }
  });
};

const convertMemoryToMessages = (memories: any[]): ChatMessage[] => {
  const validMessages = memories.map(memory => {
    // Extract artifacts from memory.analysis.artifacts
    const artifacts = memory.analysis?.artifacts || memory.artifacts;
    
    if (artifacts?.length > 0) {
      // Add each artifact to global store
      artifacts.forEach(art => addArtifact(art));
    }
    
    return {
      id: memory.id,
      role: memory.role,
      content: memory.content,
      timestamp: parseTimestamp(memory.timestamp),
      artifacts: artifacts || [],
      metadata: { ... }
    };
  });
  
  return validMessages.sort((a, b) => a.timestamp - b.timestamp);
};
```

**Artifact Restoration:**
- Artifacts are stored in memory as `analysis.artifacts`
- On load, artifacts are extracted and added to artifact store
- If artifacts found, artifact panel opens automatically after 500ms delay

---

### `useArtifacts.ts`

**Purpose:** Manage artifact panel operations

**Key Functions:**
```typescript
const saveArtifactToFile = async (id: string, filename: string) => {
  await send({
    type: 'file_system_command',
    method: 'file.save',
    params: { path: filename, content: artifact.content }
  });
};

const copyArtifact = (id: string) => {
  navigator.clipboard.writeText(artifact.content);
};

const closeArtifacts = () => {
  setShowArtifacts(false);
};
```

---

## Components

### Layout Components

#### `Header.tsx`

**Responsibilities:**
- Project selector button
- Quick file open (Cmd+P) trigger
- Git sync/commit buttons
- Artifact panel toggle
- Project context indicator

**Key Features:**
- Shows "No Project" if no project selected
- Hides Git buttons if project has no repository
- Shows artifact count badge
- Context indicator shows current project and repository status

---

#### `ChatArea.tsx`

**Simple layout component:**
```typescript
<div className="flex-1 flex flex-col min-h-0">
  <ConnectionBanner />
  <MessageList />  {/* flex-1 overflow-hidden */}
  <ChatInput />     {/* flex-shrink-0 */}
</div>
```

**Layout Strategy:**
- Uses flexbox with `min-h-0` to allow MessageList to scroll properly
- ConnectionBanner is fixed height
- MessageList is scrollable (flex-1 + overflow-hidden)
- ChatInput is fixed height (flex-shrink-0)

---

#### `ArtifactPanel.tsx`

**Monaco editor panel for viewing/editing code**

**Layout Fix (CRITICAL):**
```typescript
<div className="h-full flex flex-col">
  {/* Header: flex-shrink-0 */}
  <div className="flex-shrink-0 border-b">
    {/* Tabs and action bar */}
  </div>
  
  {/* Editor: flex-1 min-h-0 overflow-hidden */}
  <div className="flex-1 min-h-0 overflow-hidden">
    <MonacoEditor value={content} language={language} />
  </div>
</div>
```

**Why `overflow-hidden` is critical:**
- Without it, Monaco expands beyond container boundaries
- Monaco needs explicit height constraints from parent
- `flex-1 + min-h-0 + overflow-hidden` forces Monaco to respect flex container

**Features:**
- Tab interface for multiple artifacts
- Copy to clipboard
- Save to file (prompts for filename)
- Close individual tabs
- Syntax highlighting based on language

---

### Chat Components

#### `MessageList.tsx`

**Virtualized message rendering using react-virtuoso**

**Key Features:**
- Virtualized rendering for performance (only renders visible messages)
- Auto-scroll on new messages
- Auto-scroll when thinking indicator appears
- Scroll-to-bottom button when not at bottom
- Empty state when no messages

**Virtuoso Configuration:**
```typescript
<Virtuoso
  ref={virtuosoRef}
  data={messages}
  overscan={200}                    // Render 200px above/below viewport
  followOutput={false}              // Don't auto-follow (we control it)
  initialTopMostItemIndex={messages.length - 1}  // Start at bottom
  atBottomStateChange={handleAtBottomStateChange}
  atBottomThreshold={50}
  alignToBottom                     // Align to bottom when fewer items than viewport
  components={{
    Footer: () => <ThinkingIndicator isWaiting={isWaitingForResponse} />
  }}
/>
```

**Auto-scroll Logic:**
1. When message count changes → scroll to bottom after 50ms
2. When `isWaitingForResponse` becomes true → scroll to bottom after 100ms
3. On mount (with existing messages) → scroll to bottom after 200ms

**Performance:**
- Only renders messages in/near viewport
- Memoized message components prevent unnecessary re-renders
- Overscan of 200px provides smooth scrolling

---

#### `ChatMessage.tsx`

**Individual message display with artifacts**

**Features:**
- Markdown rendering with syntax highlighting
- Artifact list with Apply/View/Undo buttons
- "Apply All" button for batch file writes
- User/Assistant/System avatars
- Timestamp

**Artifact Actions:**
```typescript
// Apply single artifact
const handleApplyArtifact = async (artifact: Artifact) => {
  await send({
    type: 'file_system_command',
    method: 'files.write',
    params: {
      project_id: currentProject.id,
      path: artifact.path,
      content: artifact.content,
    }
  });
  setAppliedArtifacts(prev => new Set(prev).add(artifact.id));
};

// Apply all artifacts (batch operation)
const handleApplyAll = async () => {
  await send({
    type: 'file_system_command',
    method: 'write_files',  // Batch endpoint
    params: {
      project_id: currentProject.id,
      files: artifacts.map(a => ({ path: a.path, content: a.content }))
    }
  });
};

// Undo artifact (restore from git)
const handleUndoArtifact = async (artifact: Artifact) => {
  await send({
    type: 'git_command',
    method: 'git.restore',
    params: {
      project_id: currentProject.id,
      file_path: artifact.path,
    }
  });
};
```

**Artifact UI:**
- **Primary Fix** badge for main file in error fix workflow
- **Change type** badges (import, type, cascade)
- **Applied** checkmark when artifact has been written to disk
- **View** button opens artifact in Monaco editor
- **Apply** button writes file to disk
- **Undo** button restores from Git

---

#### `ChatInput.tsx`

**Message input with auto-resize**

**Features:**
- Auto-resizing textarea (min 48px, max 200px)
- Enter to send, Shift+Enter for new line
- Disabled when disconnected or waiting for response
- Auto-focus on connection
- Placeholder changes based on connection/project state

**Auto-resize Implementation:**
```typescript
const handleChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
  setInputContent(e.target.value);
  
  if (textareaRef.current) {
    const textarea = textareaRef.current;
    textarea.style.height = 'auto';  // Reset height
    
    // Use requestAnimationFrame to decouple from scroll events
    requestAnimationFrame(() => {
      const newHeight = Math.min(textarea.scrollHeight, 200);
      textarea.style.height = `${newHeight}px`;
    });
  }
};
```

**Why requestAnimationFrame?**
- Prevents scroll-linked effect warnings in browser
- Decouples height calculation from scroll events
- Smoother animation

---

#### `MessageBubble.tsx`

**Memoized message bubble component**

**Memoization Strategy:**
```typescript
export const MessageBubble = React.memo(MessageBubbleComponent, (prevProps, nextProps) => {
  return (
    prevProps.message.id === nextProps.message.id &&
    prevProps.message.content === nextProps.message.content &&
    prevProps.message.artifacts?.length === nextProps.message.artifacts?.length &&
    prevProps.isLast === nextProps.isLast
  );
});
```

**Why memoize?**
- Prevents re-rendering all messages when user types in ChatInput
- Only re-renders if message content/artifacts actually change
- Critical for performance with large message histories

**Features:**
- Code block parsing and syntax highlighting
- Copy button (hover to show)
- Thumbs up/down feedback buttons
- Avatar based on role (user/assistant/system)

---

### Project Management

#### `ProjectsView.tsx`

**Full-featured project management interface**

**Sections:**
1. **Project List** - Grid of project cards
2. **Create Project Modal** - Name + description
3. **Delete Confirmation Modal**
4. **Attach Local Directory Modal** - For importing existing projects
5. **Documents Modal** - Upload/search/manage documents

**Project Card Features:**
- Active project indicator
- Repository status badge
- Created date
- Tags (up to 3 visible)
- Delete button
- "Attach Local Directory" button (if no repo)
- "Documents" button (opens document manager)

**Document Management (Nested Modals):**

**Upload Tab:**
- Drag & drop file upload
- File picker button
- Progress bar during upload
- Supported formats: PDF, DOCX, DOC, TXT, MD
- Max size: 50MB

**List Tab:**
- Shows all uploaded documents
- File name, size, word count, chunk count
- Created date
- Download button
- Delete button with confirmation

**Search Tab:**
- Semantic search across all documents
- Search results with relevance scores
- Highlighted query terms in content
- Color-coded match quality (excellent/good/fair/weak)
- Score visualization bar

**Key Commands:**
```typescript
// Create project
await send({
  type: 'project_command',
  method: 'project.create',
  params: { name, description }
});

// Attach local directory
await send({
  type: 'project_command',
  method: 'project.attach_local',
  params: { project_id, directory_path }
});

// Upload document
await send({
  type: 'document_command',
  method: 'documents.upload',
  params: { project_id, file_name, content: base64 }
});

// Search documents
await send({
  type: 'document_command',
  method: 'documents.search',
  params: { project_id, query, limit: 10 }
});
```

---

#### `QuickFileOpen.tsx`

**Cmd+P file picker (VSCode-style)**

**Features:**
- Opens with Cmd+P / Ctrl+P
- Fuzzy search by filename or path
- Arrow keys to navigate
- Enter to open file
- Escape to close
- Shows file count

**Performance Fix:**
```typescript
// WRONG: causes re-render loop
useEffect(() => {
  loadFiles();
}, [currentProject]); // currentProject is an object - changes reference every update

// RIGHT: stable dependency
const projectId = currentProject?.id;
useEffect(() => {
  loadFiles();
}, [projectId]); // primitive value - stable reference
```

**File Loading:**
```typescript
await send({
  type: 'git_command',
  method: 'git.tree',
  params: { project_id, recursive: true }
});

// Flatten tree structure
const flattenFiles = (nodes: FileNode[]) => {
  const result = [];
  for (const node of nodes) {
    if (!node.is_directory) {
      result.push({ name: node.name, path: node.path, type: 'file' });
    }
    if (node.children) {
      result.push(...flattenFiles(node.children));
    }
  }
  return result;
};
```

**File Selection:**
```typescript
await send({
  type: 'git_command',
  method: 'git.file',
  params: { project_id, file_path }
});
// Response handled by useWebSocketMessageHandler → creates artifact
```

---

### Monaco Editor

#### `MonacoEditor.tsx`

**Monaco editor wrapper with improved readability**

**Key Configuration:**
```typescript
const defaultOptions = {
  // Editor behavior
  minimap: { enabled: true, maxColumn: 80 },
  lineNumbers: 'on',
  scrollBeyondLastLine: false,
  automaticLayout: true,  // Auto-resize when container changes
  tabSize: 2,
  insertSpaces: true,
  wordWrap: 'on',
  
  // READABILITY IMPROVEMENTS
  fontSize: 16,  // Increased from 14
  lineHeight: 24,
  fontFamily: "'JetBrains Mono', 'Fira Code', 'Consolas', monospace",
  fontLigatures: true,
  
  // Padding and spacing
  padding: { top: 16, bottom: 16 },
  
  // Visual
  renderLineHighlight: 'all',
  renderWhitespace: 'selection',
  cursorStyle: 'line',
  cursorBlinking: 'smooth',
  smoothScrolling: true,
  
  // Theme
  theme: 'vs-dark',
};
```

**Height Management:**
- Default: `height="100%"` (fills parent container)
- Parent must have explicit height constraint
- `automaticLayout: true` handles window resize

---

## Services

### `BackendCommands.ts`

**Unified API for backend communication**

**Categories:**
1. **Project Commands** - list, create, update, delete, attach_local
2. **Git Commands** - import, sync, commit, push, pull, status, branch, checkout, reset, restore
3. **File System Commands** - write, read, list, delete, create directory, search
4. **Code Intelligence** - search code, complexity hotspots, stats, analyze file
5. **Document Commands** - upload, search, list, retrieve, delete
6. **Chat Commands** - send message with context
7. **Memory Commands** - search memory, stats

**Example Usage:**
```typescript
const commands = new BackendCommands();

// Project operations
await commands.createProject('My Project', 'Description');
await commands.listProjects();

// Git operations
await commands.gitCommit(projectId, 'Update files', ['src/main.rs']);
await commands.gitPush(projectId);

// File operations
await commands.writeFile(projectId, 'src/lib.rs', 'pub fn hello() {}');
await commands.searchCode('fn main', projectId);

// Document operations
await commands.uploadDocument(projectId, 'doc.pdf', base64Content);
await commands.searchDocuments(projectId, 'semantic query');

// Chat with context
await commands.sendChat('Fix this bug', projectId, {
  file_path: 'src/main.rs',
  file_content: '...',
  language: 'rust'
});
```

---

## Types & Interfaces

### `src/types/index.ts`

**Core Types:**

```typescript
// ===== PROJECT =====
interface Project {
  id: string;
  name: string;
  description?: string;
  tags?: string[];
  owner?: string;
  has_repository?: boolean;
  repository_url?: string;
  import_status?: string;
  last_sync_at?: string | null;
  created_at: string;  // RFC3339 timestamp
  updated_at: string;
}

// ===== DOCUMENTS =====
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
  score: number;  // 0.0 to 1.0
  page_number?: number;
}

// ===== FILE SYSTEM =====
interface FileNode {
  name: string;
  path: string;
  type: 'file' | 'directory';
  children?: FileNode[];
  size?: number;
  modified?: string;
}

// ===== TOOL RESULTS =====
interface ToolResult {
  id: string;
  type: 'web_search' | 'code_execution' | 'file_operation' | 'git_operation' | 'code_analysis' | 'code_search' | 'repository_stats' | 'complexity_analysis';
  status: 'success' | 'error' | 'pending';
  data: any;
  timestamp: number;
}

// ===== WEBSOCKET =====
interface WebSocketMessage {
  type: string;
  [key: string]: any;
}
```

**All types are exported from single file for consistency**

---

## WebSocket Protocol

### Message Types

#### Outgoing (Frontend → Backend)

```typescript
// Chat message
{
  type: 'chat',
  content: string,
  project_id?: string,
  metadata: {
    session_id: string,
    timestamp: number,
    file_path?: string,
    file_content?: string,
    language?: string,
    has_repository: boolean,
    current_branch: string,
    modified_files_count: number,
  }
}

// Project command
{
  type: 'project_command',
  method: 'project.list' | 'project.create' | 'project.delete' | ...,
  params: { ... }
}

// Git command
{
  type: 'git_command',
  method: 'git.commit' | 'git.push' | 'git.tree' | 'git.file' | ...,
  params: { ... }
}

// File system command
{
  type: 'file_system_command',
  method: 'files.write' | 'files.read' | 'write_files' | ...,
  params: { ... }
}

// Document command
{
  type: 'document_command',
  method: 'documents.upload' | 'documents.search' | 'documents.list' | ...,
  params: { ... }
}

// Memory command
{
  type: 'memory_command',
  method: 'memory.get_recent' | 'memory.search' | 'memory.stats',
  params: { ... }
}
```

#### Incoming (Backend → Frontend)

```typescript
// Chat response
{
  type: 'response',
  streaming?: boolean,     // If true, content is incremental
  complete?: boolean,      // If true, streaming is done
  content?: string,        // Message content
  data?: {
    content: string,
    artifacts?: Artifact[],
    thinking?: string,
  }
}

// Data message
{
  type: 'data',
  data: {
    type: 'project_list' | 'file_tree' | 'file_content' | 'document_list' | ...,
    // ... type-specific data
  }
}

// Status message
{
  type: 'status',
  message: string,
}

// Error message
{
  type: 'error',
  message: string,
  error?: string,
}

// Heartbeat (keep-alive)
{
  type: 'heartbeat'
}
```

### Message Routing

**WebSocketStore Subscriber System:**

```typescript
// Filtered subscription (only specific message types)
subscribe('chat-handler', handleChatResponse, ['response']);

// Global subscription (all message types)
subscribe('global-handler', handleAllMessages);
```

**Message Flow:**

```
┌─────────────────────────────────────────┐
│        WebSocket Message                 │
└─────────────────┬───────────────────────┘
                  │
                  ↓
┌─────────────────────────────────────────┐
│  useWebSocketStore.handleMessage()      │
│  - Sets lastMessage                      │
│  - Logs message                          │
│  - Notifies filtered subscribers         │
└─────────────────┬───────────────────────┘
                  │
         ┌────────┴────────┐
         ↓                 ↓
┌──────────────┐  ┌──────────────────┐
│ Listener 1   │  │ Listener 2       │
│ Types: ['A'] │  │ Types: undefined │
│              │  │ (receives all)   │
└──────────────┘  └──────────────────┘
```

---

## Data Flow

### Message Send Flow

```
┌────────────────────────────────────────────────┐
│ User types message + presses Enter            │
└────────────────┬───────────────────────────────┘
                 │
                 ↓
┌────────────────────────────────────────────────┐
│ ChatInput.handleSend()                         │
│ - Validates connection state                   │
│ - Clears input field                           │
└────────────────┬───────────────────────────────┘
                 │
                 ↓
┌────────────────────────────────────────────────┐
│ useChatMessaging.handleSend()                  │
│ 1. Add user message to useChatStore           │
│ 2. Set isWaitingForResponse = true            │
│ 3. Build message with context:                │
│    - project_id, session_id                   │
│    - file_path, file_content (if artifact)    │
│    - has_repository, current_branch           │
│    - modified_files_count                     │
│ 4. Send via WebSocket                         │
└────────────────┬───────────────────────────────┘
                 │
                 ↓
┌────────────────────────────────────────────────┐
│ useWebSocketStore.send()                       │
│ - If connected: send immediately               │
│ - If disconnected: queue message               │
└────────────────┬───────────────────────────────┘
                 │
                 ↓
┌────────────────────────────────────────────────┐
│ WebSocket.send(JSON.stringify(message))       │
└────────────────────────────────────────────────┘
```

### Message Receive Flow

```
┌────────────────────────────────────────────────┐
│ Backend sends WebSocket message               │
└────────────────┬───────────────────────────────┘
                 │
                 ↓
┌────────────────────────────────────────────────┐
│ WebSocket.onmessage event                      │
│ - Parse JSON                                   │
└────────────────┬───────────────────────────────┘
                 │
                 ↓
┌────────────────────────────────────────────────┐
│ useWebSocketStore.handleMessage()              │
│ 1. Set lastMessage                             │
│ 2. Log message (if not silent type)            │
│ 3. Notify filtered subscribers                 │
└────────────────┬───────────────────────────────┘
                 │
         ┌───────┴──────────┐
         │                  │
         ↓                  ↓
┌──────────────────┐  ┌────────────────────────┐
│ useMessage       │  │ useWebSocketMessage    │
│ Handler          │  │ Handler                │
│                  │  │                        │
│ type='response'  │  │ type='data'            │
│ → Chat messages  │  │ → Projects, Files, Git │
└──────────────────┘  └────────────────────────┘
         │                  │
         ↓                  ↓
┌──────────────────┐  ┌────────────────────────┐
│ useChatStore     │  │ useAppState            │
│ - Add message    │  │ - Update projects      │
│ - Add artifacts  │  │ - Update git status    │
│ - End waiting    │  │ - Create artifacts     │
└──────────────────┘  └────────────────────────┘
         │                  │
         ↓                  ↓
┌────────────────────────────────────────────────┐
│ React re-renders affected components          │
└────────────────────────────────────────────────┘
```

### Chat History Load Flow

```
┌────────────────────────────────────────────────┐
│ App.tsx mounts                                 │
└────────────────┬───────────────────────────────┘
                 │
                 ↓
┌────────────────────────────────────────────────┐
│ useWebSocketStore.connect()                    │
│ - connectionState = 'connecting'               │
│ - connectionState = 'connected' (on open)      │
└────────────────┬───────────────────────────────┘
                 │
                 ↓
┌────────────────────────────────────────────────┐
│ useChatPersistence detects connection          │
│ - Wait 100ms for connection to settle          │
└────────────────┬───────────────────────────────┘
                 │
                 ↓
┌────────────────────────────────────────────────┐
│ useChatPersistence.loadChatHistory()           │
│ - Send: type='memory_command'                  │
│         method='memory.get_recent'             │
│         params={ session_id, count: 100 }      │
└────────────────┬───────────────────────────────┘
                 │
                 ↓
┌────────────────────────────────────────────────┐
│ Backend responds with type='data'              │
│ data.memories = [ ... array of messages ... ]  │
└────────────────┬───────────────────────────────┘
                 │
                 ↓
┌────────────────────────────────────────────────┐
│ useChatPersistence.convertMemoryToMessages()   │
│ 1. Map memories to ChatMessage format          │
│ 2. Extract artifacts from memory.analysis      │
│ 3. Add artifacts to useAppState                │
│ 4. Sort messages by timestamp                  │
│ 5. Deduplicate based on content + time         │
└────────────────┬───────────────────────────────┘
                 │
                 ↓
┌────────────────────────────────────────────────┐
│ useChatStore.setMessages(loadedMessages)       │
│ - Replaces entire message array                │
└────────────────┬───────────────────────────────┘
                 │
                 ↓
┌────────────────────────────────────────────────┐
│ If artifacts found:                            │
│ - Wait 500ms                                   │
│ - useAppState.setShowArtifacts(true)           │
└────────────────────────────────────────────────┘
                 │
                 ↓
┌────────────────────────────────────────────────┐
│ MessageList renders with history + artifacts   │
└────────────────────────────────────────────────┘
```

---

## Key Features

### 1. **Real-time Streaming Responses**

```typescript
// Backend sends streaming chunks
{ type: 'response', streaming: true, content: 'chunk1' }
{ type: 'response', streaming: true, content: 'chunk2' }
{ type: 'response', complete: true }

// Frontend accumulates in real-time
startStreaming();                 // Initialize streaming state
appendStreamContent('chunk1');    // Add to buffer
appendStreamContent('chunk2');    // Add to buffer
endStreaming();                   // Finalize message
```

**Why streaming?**
- Shows AI response immediately as it's generated
- Better UX - user sees progress
- Reduces perceived latency

---

### 2. **Artifact System**

**What are artifacts?**
- Code snippets generated by AI
- Files loaded from project
- Error fixes with multiple file changes

**Artifact Lifecycle:**
1. **Creation** - AI generates artifacts or user opens file
2. **Display** - Shows in chat message with "View" button
3. **Edit** - Opens in Monaco editor in ArtifactPanel
4. **Apply** - Writes to filesystem via backend
5. **Undo** - Restores from Git

**Artifact Types (changeType):**
- `primary` - Main file being fixed
- `import` - Import/dependency changes
- `type` - Type definition changes
- `cascade` - Related file changes

**Multi-file Fix Workflow:**
```typescript
// Backend sends error fix with multiple artifacts
{
  type: 'response',
  data: {
    content: 'Here's the fix for the compilation error:',
    artifacts: [
      { id: '1', path: 'src/main.rs', content: '...', changeType: 'primary' },
      { id: '2', path: 'src/lib.rs', content: '...', changeType: 'import' },
      { id: '3', path: 'src/types.rs', content: '...', changeType: 'type' },
    ]
  }
}

// User can:
// 1. View each artifact individually
// 2. Apply artifacts one at a time
// 3. Apply all artifacts at once (batch write_files)
// 4. Undo individual artifacts (git restore)
```

---

### 3. **Project Context Awareness**

**Frontend enriches every chat message with context:**

```typescript
metadata: {
  session_id: 'peter-eternal',
  timestamp: Date.now(),
  
  // FILE CONTEXT (from active artifact)
  file_path: 'src/main.rs',
  file_content: '... full file content ...',
  language: 'rust',
  
  // PROJECT CONTEXT (from useAppState)
  has_repository: true,
  current_branch: 'feature/new-api',
  modified_files_count: 3,
}
```

**Why this matters:**
- AI knows which file you're looking at
- AI knows if project has Git repo (can suggest Git commands)
- AI knows current branch (can suggest branch-specific operations)
- AI knows you have uncommitted changes

---

### 4. **Persistent Chat History**

**Storage Layers:**
1. **Backend Memory System** - Stores all messages with embeddings
2. **Frontend localStorage** - Caches recent messages for instant load
3. **WebSocket Sync** - Loads full history from backend on connect

**On page load:**
1. Restore messages from localStorage (instant)
2. Connect to WebSocket
3. Load full history from backend (100 most recent)
4. Merge backend history with localStorage
5. Restore artifacts from history

**Artifact Restoration:**
- Artifacts are stored in memory as `analysis.artifacts`
- On load, artifacts are extracted and added to artifact panel
- If artifacts found, panel opens automatically

---

### 5. **Git Integration**

**Git Commands Available:**
- `git.status` - Check for modified files
- `git.commit` - Commit changes
- `git.push` - Push to remote
- `git.pull` - Pull from remote
- `git.restore` - Restore file from Git
- `git.branch` - List/create branches
- `git.checkout` - Switch branches
- `git.sync` - Auto-commit + push

**UI Integration:**
- GitSyncButton - Quick sync button in header
- CommitPushButton - Commit with message + push
- Modified files badge in header
- Undo button in artifact UI (uses git.restore)

---

### 6. **Document Management**

**Features:**
- Upload PDF, DOCX, DOC, TXT, MD files
- Max 50MB per file
- Automatic chunking on backend
- Semantic search across all documents
- View document metadata (size, word count, chunks)
- Download documents
- Delete documents

**Semantic Search:**
- Uses embeddings (not keyword search)
- Returns relevance score (0.0 to 1.0)
- Shows matching chunks with context
- Highlights query terms in results
- Color-coded match quality

---

### 7. **Quick File Open (Cmd+P)**

**Features:**
- Fuzzy search by filename or path
- Arrow keys to navigate results
- Enter to open file in artifact panel
- Shows file count
- Only shows files (not directories)
- Only available when project has repository

**Performance:**
- Loads file tree once when opened
- Filters in frontend (no backend requests during typing)
- Flattens tree structure for fast search

---

## Performance Optimizations

### 1. **WebSocket Subscriber Filtering**

**Problem:** Every component was re-rendering on every WebSocket message

**Solution:** Filtered subscriptions
```typescript
// OLD: receives ALL messages (unnecessary re-renders)
subscribe('handler', handleMessage);

// NEW: only receives specific message types
subscribe('chat-handler', handleChatResponse, ['response']);
subscribe('doc-upload', handleDocProgress, ['data', 'error']);
```

**Impact:**
- Reduced unnecessary re-renders by 90%
- Only relevant components process messages

---

### 2. **Zustand Selector Optimization**

**Problem:** Entire components re-rendered when unrelated state changed

**Solution:** Selective subscriptions with optimized selectors
```typescript
// BAD: subscribes to entire store
const { inputContent, activeTab, activeModal } = useUIStore();
// Component re-renders when ANY UI state changes

// GOOD: subscribe to specific values
const inputContent = useUIStore(state => state.inputContent);
const activeTab = useUIStore(state => state.activeTab);
// Component only re-renders when inputContent changes
```

**Custom selector hooks:**
```typescript
export const useInputContent = () => useUIStore(state => state.inputContent);
export const useActiveTab = () => useUIStore(state => state.activeTab);
```

---

### 3. **React.memo for MessageBubble**

**Problem:** All messages re-rendered when user typed in input field

**Solution:** Memoize MessageBubble component
```typescript
export const MessageBubble = React.memo(MessageBubbleComponent, (prev, next) => {
  return (
    prev.message.id === next.message.id &&
    prev.message.content === next.message.content &&
    prev.message.artifacts?.length === next.message.artifacts?.length
  );
});
```

**Impact:**
- Messages only re-render when content/artifacts change
- Typing in input field doesn't re-render message list

---

### 4. **Virtualized Message List**

**Problem:** Rendering 1000+ messages causes performance issues

**Solution:** Use react-virtuoso for virtualized rendering
```typescript
<Virtuoso
  data={messages}
  overscan={200}  // Render 200px above/below viewport
  itemContent={(index, message) => <ChatMessage message={message} />}
/>
```

**Impact:**
- Only renders visible messages + overscan buffer
- Smooth scrolling even with thousands of messages
- Constant memory usage regardless of message count

---

### 5. **Stable Dependencies in useEffect**

**Problem:** Effect running on every render due to object reference changes

**Solution:** Extract primitive values for dependencies
```typescript
// BAD: currentProject is object - changes reference often
useEffect(() => {
  loadFiles();
}, [currentProject]);

// GOOD: primitive value - stable reference
const projectId = currentProject?.id;
useEffect(() => {
  loadFiles();
}, [projectId]);
```

---

### 6. **Auto-resize Textarea with requestAnimationFrame**

**Problem:** Scroll-linked effect warnings when resizing textarea

**Solution:** Decouple from scroll events using RAF
```typescript
const handleChange = (e) => {
  setInputContent(e.target.value);
  
  textarea.style.height = 'auto';
  
  // Decouple from scroll events
  requestAnimationFrame(() => {
    const newHeight = Math.min(textarea.scrollHeight, 200);
    textarea.style.height = `${newHeight}px`;
  });
};
```

---

### 7. **Batch File Writes**

**Problem:** Applying 10 artifacts = 10 WebSocket messages = potential message loss

**Solution:** Use `write_files` endpoint for batch operations
```typescript
// BAD: sends 10 separate messages
for (const artifact of artifacts) {
  await send({ type: 'file_system_command', method: 'files.write', ... });
}

// GOOD: sends 1 message with all files
await send({
  type: 'file_system_command',
  method: 'write_files',
  params: {
    project_id,
    files: artifacts.map(a => ({ path: a.path, content: a.content }))
  }
});
```

---

## Development Guidelines

### State Management Rules

1. **Use Zustand stores for shared state**
   - Projects, artifacts, messages, UI state

2. **Use local useState for component-specific state**
   - Form inputs, modals, loading states

3. **Persist critical data to localStorage**
   - Projects, artifacts, messages
   - NOT UI state, streaming state, or temporary data

4. **Use optimized selectors**
   - Subscribe to specific values, not entire store
   - Create custom selector hooks for common patterns

---

### Component Design Rules

1. **Memoize expensive components**
   - Message bubbles, code blocks, lists

2. **Extract primitive dependencies**
   - Use `project.id` not `project` in useEffect deps

3. **Use flex layout with explicit constraints**
   - `h-screen`, `h-full`, `flex-1`, `min-h-0`
   - Monaco editor needs `overflow-hidden` on parent

4. **Keep components focused**
   - Single responsibility
   - Extract complex logic to hooks

---

### WebSocket Best Practices

1. **Filter subscriptions**
   - Only subscribe to message types you need
   - Reduces unnecessary processing

2. **Handle disconnection gracefully**
   - Message queue for offline messages
   - Auto-reconnect with exponential backoff

3. **Don't assume connection state**
   - Check `connectionState === 'connected'` before sending

4. **Use type-safe message structures**
   - Define interfaces for all message types
   - Validate incoming messages

---

### Performance Best Practices

1. **Virtualize long lists**
   - Use react-virtuoso for messages, files, documents

2. **Debounce expensive operations**
   - Search queries, file tree filtering

3. **Use requestAnimationFrame for layout changes**
   - Prevents scroll-linked warnings

4. **Batch operations when possible**
   - Use batch endpoints for multiple files

5. **Monitor WebSocket message volume**
   - Add debug logging for message counts
   - Identify chatty subscribers

---

### Git Workflow

1. **Check for uncommitted changes before operations**
   - Show warning if modified files exist

2. **Provide undo for destructive operations**
   - Use `git.restore` to revert file changes

3. **Auto-commit with descriptive messages**
   - Include file count, change type in commit message

4. **Show repository status in UI**
   - Modified files count
   - Current branch
   - Sync status

---

### Artifact Workflow

1. **Always provide complete file content**
   - Never use "..." or "rest unchanged" in artifacts

2. **Support multiple artifact types**
   - Primary fix, imports, types, cascade changes

3. **Track which artifacts have been applied**
   - Use Set in useAppState
   - Show checkmarks for applied artifacts

4. **Open artifact panel automatically**
   - When artifacts are created/loaded from history

---

## File Manifest

### Root Level
- `index.html` - HTML entry point
- `package.json` - Dependencies and scripts
- `package-lock.json` - Locked dependency versions
- `tsconfig.json` - TypeScript configuration
- `tsconfig.node.json` - TypeScript config for Vite
- `vite.config.js` - Vite build configuration
- `tailwind.config.js` - Tailwind CSS configuration
- `postcss.config.js` - PostCSS configuration
- `.gitignore` - Git ignore rules
- `LICENSE` - MIT License
- `README.md` - Project documentation

### Source Files (src/)

**Core:**
- `main.tsx` - React entry point (9 lines)
- `App.tsx` - Root component (112 lines)
- `App.css` - Global styles
- `index.css` - Tailwind imports
- `vite-env.d.ts` - Vite type definitions

**Components (src/components/):**
- `ArtifactPanel.tsx` - Code/file viewer with Monaco (5173 bytes)
- `ArtifactToggle.tsx` - Toggle button for artifact panel
- `ChatArea.tsx` - Main chat container (21 lines)
- `ChatInput.tsx` - Message input field (100 lines)
- `ChatMessage.tsx` - Individual message display (307 lines)
- `CodeBlock.tsx` - Syntax-highlighted code blocks (81 lines)
- `CommitPushButton.tsx` - Git commit/push UI
- `ConnectionBanner.tsx` - WebSocket status indicator
- `FileBrowser.tsx` - File tree navigation
- `GitSyncButton.tsx` - Git sync operations
- `Header.tsx` - Top navigation bar (100 lines)
- `MessageBubble.tsx` - Chat message bubble (216 lines)
- `MessageList.tsx` - Virtualized message list (126 lines)
- `MonacoEditor.tsx` - Monaco editor wrapper (73 lines)
- `ProjectsView.tsx` - Project management UI (1176 lines)
- `QuickFileOpen.tsx` - Cmd+P file picker (317 lines)
- `ThinkingIndicator.tsx` - Loading/thinking animation

**Components - Documents (src/components/documents/):**
- `DocumentsView.tsx` - Main documents UI
- `DocumentSearch.tsx` - Document search interface
- `DocumentUpload.tsx` - File upload component
- `DocumentList.tsx` - Document list display
- `index.ts` - Barrel exports

**Components - Modals (src/components/modals/):**
- `FileSearchModal.tsx` - File search modal
- `ImageGenerationModal.tsx` - Image generation UI
- `index.ts` - Barrel exports

**Hooks (src/hooks/):**
- `useArtifacts.ts` - Artifact management (85 lines)
- `useChatMessaging.ts` - Send messages with context (124 lines)
- `useChatPersistence.ts` - Load/restore chat history (243 lines)
- `useMessageHandler.ts` - Process incoming messages (99 lines)
- `useWebSocketMessageHandler.ts` - WebSocket message routing (214 lines)

**Stores (src/stores/):**
- `useAppState.ts` - Global app state (231 lines)
- `useAuthStore.ts` - Authentication (69 lines)
- `useChatStore.ts` - Chat messages (129 lines)
- `useUIStore.ts` - Transient UI state (48 lines)
- `useWebSocketStore.ts` - WebSocket connection (290 lines)

**Services (src/services/):**
- `BackendCommands.ts` - Backend command API (394 lines)

**Types (src/types/):**
- `index.ts` - All shared types (75 lines)

---

## Summary

Mira Frontend is a sophisticated, real-time chat interface built with modern web technologies and best practices:

- **Real-time WebSocket communication** with filtered subscriptions for performance
- **Persistent state management** using Zustand with localStorage
- **Code editing capabilities** with Monaco Editor
- **Artifact system** for managing code/files generated by AI
- **Project-based workflow** with Git integration
- **Document management** with semantic search
- **Virtualized rendering** for performance with large datasets
- **Optimized re-rendering** with memoization and selective subscriptions
- **Complete chat history** restored from backend memory system

The architecture is designed for **maintainability**, **performance**, and **developer experience**, with clear separation of concerns, type safety, and comprehensive state management.
