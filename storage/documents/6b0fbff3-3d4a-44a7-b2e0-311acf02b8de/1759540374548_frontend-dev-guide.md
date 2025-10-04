# Mira Frontend - Developer Navigation Guide
## Architecture & Technology Stack Overview - October 2025

### Core Technologies
- **React 18** - UI framework
- **TypeScript** - Type safety
- **Zustand** - State management (replaced Redux/Context)
- **Vite 7.1.4** - Build tool & dev server
- **Tailwind CSS** - Styling
- **Monaco Editor** - Code editing
- **WebSocket** - Real-time backend communication

### Key Libraries
- `@monaco-editor/react` (^4.7.0) - VS Code editor integration
- `react-markdown` (^10.1.0) - Markdown rendering
- `react-syntax-highlighter` (^15.6.6) - Code syntax highlighting
- `lucide-react` (^0.365.0) - Icon library
- `clsx` (^2.1.0) - Conditional className utility
- `ws` (^8.18.3) - WebSocket client

---

## Directory Structure & Navigation Map

```
mira-frontend/ (399K total, 35 files, 7 directories)
├── src/
│   ├── App.tsx (1.7K)          # Main app container & layout orchestration
│   ├── main.tsx (216B)         # React entry point
│   ├── index.css (845B)        # Global styles & Tailwind imports
│   ├── App.css (2.3K)          # App-specific animations & styles
│   ├── vite-env.d.ts (253B)    # Vite environment types
│   │
│   ├── stores/ (21K)           # 🎯 Zustand State Management
│   │   ├── useWebSocketStore.ts (5.7K)  # WebSocket connection & messaging
│   │   ├── useChatStore.ts (5.3K)       # Chat messages & streaming
│   │   └── useAppState.ts (5.6K)        # Global app state (projects, artifacts, UI)
│   │
│   ├── hooks/ (32K)            # 🪝 Custom React Hooks
│   │   ├── useMessageHandler.ts (5.8K)  # WebSocket message processing
│   │   ├── useWebSocketMessageHandler.ts (8.5K)  # Global message router
│   │   ├── useArtifacts.ts (2.4K)       # Artifact CRUD operations
│   │   ├── useChatMessaging.ts (5.7K)   # Chat message sending logic
│   │   └── useChatPersistence.ts (6.0K) # Chat history persistence
│   │
│   ├── components/ (119K)      # 🧩 React Components
│   │   ├── Header.tsx (2.9K)            # Top navigation bar
│   │   ├── ChatContainer.tsx (6.4K)     # Main chat interface
│   │   ├── ChatMessage.tsx (9.6K)       # Individual message display
│   │   ├── ChatInput.tsx (3.0K)         # Message input field
│   │   ├── MessageList.tsx (1.7K)       # Message list container
│   │   ├── MessageBubble.tsx (7.3K)     # Message bubble wrapper
│   │   ├── ThinkingIndicator.tsx (1.3K) # AI thinking animation
│   │   ├── CodeBlock.tsx (2.2K)         # Code display component
│   │   ├── ArtifactPanel.tsx (8.1K)     # Code editor sidebar
│   │   ├── ArtifactToggle.tsx (1.5K)    # Sidebar toggle button
│   │   ├── MonacoEditor.tsx (1021B)     # Monaco wrapper component
│   │   ├── QuickFileOpen.tsx (9.3K)     # Cmd+P file browser modal
│   │   ├── ProjectDropdown.tsx (18K)    # Project selector
│   │   ├── FileBrowser.tsx (5.5K)       # File tree navigator
│   │   └── GitSyncButton.tsx (3.2K)     # Git sync operations
│   │
│   ├── services/ (8.7K)        # 🔧 Service Layer
│   │   └── BackendCommands.ts (8.7K)    # WebSocket command builders
│   │
│   └── types/ (1.5K)           # 📝 TypeScript Definitions
│       └── index.ts (1.5K)              # Shared type definitions
```

---

## State Management (Zustand Stores)

### WebSocketStore (`stores/useWebSocketStore.ts`)
- **Purpose**: Manages WebSocket connection and message passing
- **Key State**:
  - `connectionState`: 'disconnected' | 'connecting' | 'connected'
  - `ws`: WebSocket instance
  - `messageQueue`: Pending messages during reconnection
  - `listeners`: Map of subscriber callbacks
- **Key Actions**:
  - `connect()`: Establish WebSocket connection
  - `disconnect()`: Close connection
  - `send(message)`: Send message with queueing
  - `subscribe(id, callback)`: Register message listener
  - `unsubscribe(id)`: Remove listener
- **Features**:
  - Auto-reconnect with exponential backoff
  - Message queueing during disconnect
  - Multiple subscriber support
  - Heartbeat ping/pong

### ChatStore (`stores/useChatStore.ts`)
- **Purpose**: Chat message state and streaming
- **Key State**:
  - `messages`: Array of chat messages
  - `currentSessionId`: Active session ID
  - `isStreaming`: Streaming status
  - `streamingContent`: Accumulated stream content
  - `streamingMessageId`: ID of streaming message
- **Key Actions**:
  - `addMessage(message)`: Add new message
  - `updateMessage(id, updates)`: Update existing message
  - `setMessages(messages)`: Replace all messages
  - `startStreaming()`: Begin stream
  - `appendStreamContent(content)`: Add to stream
  - `endStreaming()`: Complete stream
  - `markArtifactApplied(msgId, artifactId)`: Mark fix as applied

### AppState (`stores/useAppState.ts`)
- **Purpose**: Global application state
- **Key State**:
  - `currentProject`: Active project
  - `projects`: All projects list
  - `artifacts`: Code/document artifacts
  - `showArtifacts`: UI visibility states
  - `modifiedFiles`: Git tracking
  - `activeArtifactId`: Currently viewed artifact
- **Key Actions**:
  - `setCurrentProject()`: Switch projects
  - `addArtifact()`: Create new artifact
  - `updateArtifact()`: Modify artifact
  - `deleteArtifact()`: Remove artifact
  - `setShowArtifacts()`: Toggle panel visibility
  - `updateGitStatus()`: Update repository state
- **Convenience Hooks**:
  - `useProjectState()`: Project-specific state
  - `useArtifactState()`: Artifact management
  - `usePersonalityState()`: AI persona settings

---

## Key Components Deep Dive

### ChatContainer.tsx
- Main chat interface
- Message display and input
- Connection status indicator
- Auto-scroll on new messages
- Integration with ChatStore and WebSocketStore

### ChatMessage.tsx
- Individual message rendering
- Markdown support via react-markdown
- Code blocks with syntax highlighting
- Artifact display with Apply/Undo buttons
- Tool execution display (planned)

### ArtifactPanel.tsx
- Monaco editor integration
- Multi-tab artifact management
- Apply/Undo operations
- Copy to clipboard
- Edit/Preview modes
- Live editing with auto-save
- Language detection for syntax highlighting

### QuickFileOpen.tsx
- Cmd+P keyboard shortcut
- Fuzzy file search
- Git tree integration
- Direct file opening in artifact viewer
- Modal with keyboard navigation

### ProjectDropdown.tsx
- Project list display
- Create new project
- Switch active project
- Delete project with confirmation
- Repository attachment status
- Git URL display

---

## Custom Hooks

### useChatMessaging.ts
- Enhanced message sending with full context
- Captures active artifact state
- Includes file content, language, project info
- Automatic language detection
- Session management

### useMessageHandler.ts
- Handles 'response' type WebSocket messages
- Chat response processing
- Streaming support
- Artifact extraction
- Thinking display

### useWebSocketMessageHandler.ts
- Global message router for all WebSocket types
- Handles: data, status, error messages
- Project list updates
- Git status updates
- File tree responses
- Document search results

### useChatPersistence.ts
- Loads chat history from backend
- Memory to message conversion
- Deduplication logic
- Session history management

### useArtifacts.ts
- Artifact CRUD operations
- Create, update, delete artifacts
- Artifact state management
- Integration with AppState

---

## WebSocket Communication

### Message Flow
```
Backend → WebSocket → handleMessage() → listeners.forEach() → Component Updates
```

### Incoming Message Types
1. **response** - Chat/AI responses (content, artifacts, thinking)
2. **data** - Backend data (projects, files, git_status, file_tree)
3. **status** - Operation status updates
4. **error** - Error messages

### Outgoing Message Types
1. **chat** - User messages with metadata
2. **project_command** - Project operations
3. **git_command** - Git operations
4. **file_system_command** - File operations
5. **code_intelligence** - Code analysis
6. **memory_command** - Memory operations
7. **document_command** - Document operations

---

## Development Patterns

### Store Subscription Pattern
```typescript
useEffect(() => {
  const unsubscribe = subscribe('component-id', (message) => {
    // Handle message
  });
  return unsubscribe;
}, [subscribe]);
```

### Accessing Store State
```typescript
// In component
const { send } = useWebSocketStore();
const { messages } = useChatStore();
const { currentProject } = useAppState();

// Outside component
const send = useWebSocketStore.getState().send;
```

### Conditional Rendering
```typescript
{currentProject?.hasRepository && <GitSyncButton />}
```

---

## Error-to-Fix Pipeline

1. User pastes error in chat
2. Backend detects error via code_fix_processor.rs
3. Backend generates complete fixed file
4. Response includes artifacts array
5. ChatMessage displays Apply/Undo buttons
6. User clicks Apply → files.write command sent
7. Backend updates file and confirms

---

## Git Integration

- **Import**: git.import → Clones and analyzes
- **Sync**: git.sync → Pull + Commit + Push
- **Status**: Real-time modified files
- **Undo**: git.restore → Revert changes
- **Tree**: git.tree → File structure for Cmd+P

---

## Future Enhancements

### In Progress
- Tool execution display
- TypeScript/JavaScript AST parsing
- Cross-language dependencies

### Planned
- Multi-file artifacts
- Real-time collaboration
- Theme customization
- Keyboard shortcuts
- Plugin architecture
- Mobile responsive
- Offline mode
- Voice input

---

*Last Updated: October 3, 2025*  
*Version: 1.0.1*  
*Status: Production Ready*