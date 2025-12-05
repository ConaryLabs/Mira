// src/stores/useWebSocketStore.ts
// ENHANCED: Added error handling, reset(), sendMessage() alias, better JSON validation

import { create } from 'zustand';
import { subscribeWithSelector } from 'zustand/middleware';

interface WebSocketMessage {
  type: string;
  [key: string]: any;
}

interface Subscriber {
  callback: (message: WebSocketMessage) => void;
  messageTypes?: string[];
}

interface WebSocketStore {
  socket: WebSocket | null;
  connectionState: 'connecting' | 'connected' | 'reconnecting' | 'disconnected' | 'error';
  reconnectAttempts: number;
  maxReconnectAttempts: number;
  reconnectDelay: number;
  lastMessage: WebSocketMessage | null;
  messageQueue: WebSocketMessage[];
  listeners: Map<string, Subscriber>;
  isConnected: boolean; // Convenience property
  
  connect: (url?: string) => void;
  disconnect: () => void;
  send: (message: any) => Promise<void>;
  sendMessage: (message: any) => Promise<void>; // Alias for send()
  subscribe: (
    id: string, 
    callback: (message: WebSocketMessage) => void,
    messageTypes?: string[]
  ) => () => void;
  
  setConnectionState: (state: WebSocketStore['connectionState']) => void;
  handleMessage: (message: WebSocketMessage) => void;
  processMessageQueue: () => void;
  scheduleReconnect: () => void;
  reset: () => void; // NEW: Reset store state
}

// Dynamically determine WebSocket URL based on current host
function getDefaultWsUrl(): string {
  if (typeof window === 'undefined') {
    return 'ws://localhost:3001/ws';
  }

  const { protocol, host } = window.location;

  // If running on localhost, use the backend port directly
  if (host.startsWith('localhost') || host.startsWith('127.0.0.1')) {
    return 'ws://localhost:3001/ws';
  }

  // For remote hosts, use the same host with WebSocket protocol
  // The reverse proxy (nginx) should route /ws to the backend
  const wsProtocol = protocol === 'https:' ? 'wss:' : 'ws:';
  return `${wsProtocol}//${host}/ws`;
}

const WS_URL = import.meta.env.VITE_WS_URL || getDefaultWsUrl();

// Helper to build WebSocket URL with authentication token
function buildWsUrl(baseUrl: string): string {
  // Lazy import to avoid circular dependency
  const authStore = (window as any).__authStore;

  // Recalculate base URL in case it wasn't available at module load time
  const effectiveBaseUrl = baseUrl === 'ws://localhost:3001/ws' ? getDefaultWsUrl() : baseUrl;

  if (!authStore) {
    // First time - store not initialized yet
    return effectiveBaseUrl;
  }

  const token = authStore.getState?.().token;
  if (token) {
    const url = new URL(effectiveBaseUrl, window.location.origin);
    url.searchParams.set('token', token);
    return url.toString().replace(/^http/, 'ws');
  }

  return effectiveBaseUrl;
}

// Message types we explicitly handle
const KNOWN_MESSAGE_TYPES = new Set([
  'status',
  'stream',
  'chat_complete',
  'connection_ready',
  'heartbeat',
  'response',
  'data',
  'error',
  // Terminal events (top-level)
  'terminal_output',
  'terminal_command_complete',
  'terminal_closed',
  'terminal_error',
  // Sudo approval events
  'sudo_approval_required',
  'sudo_approval_response',
  // Operation engine events (can be top-level)
  'operation.started',
  'operation.streaming',
  'operation.delegated',
  'operation.artifact_preview',
  'operation.artifact_completed',
  'operation.completed',
  'operation.failed',
  'operation.status_changed',
  'operation.tool_executed',
]);

const KNOWN_DATA_TYPES = new Set([
  // NEW: Streaming protocol
  'status',           // Status updates (thinking, typing)
  'stream',           // Token streaming (deltas)
  'chat_complete',    // Finalization message

  // Project/file management
  'project_list',
  'projects',
  'project_updated',
  'project_created',
  'local_directory_attached',
  'git_status',
  'file_tree',
  'file_content',

  // Documents
  'document_list',
  'document_deleted',
  'document_processing_started',
  'document_processing_progress',
  'document_processed',
  'document_content',

  // Memory
  'memory_data',

  // Terminal
  'terminal_started',
  'terminal_output',
  'terminal_closed',
  'terminal_error',
  'terminal_sessions',

  // Legacy streaming
  'stream_delta',
  'reasoning_delta',
  'stream_done',
  'artifact_created',
  'tool_result',

  // Code intelligence data types
  'budget_status',
  'semantic_search_results',
  'cochange_suggestions',
  'expertise_results',
  'code_search_results',

  // Sudo approval data types
  'sudo_pending_approvals',
  'sudo_permissions',
  'sudo_permission_added',
  'sudo_permission_toggled',
  'sudo_permission_updated',
  'sudo_blocklist',
  'sudo_blocklist_added',
  'sudo_blocklist_toggled',
  'sudo_audit_log',

  // Operation engine events (can also be wrapped in data envelope)
  'operation.started',
  'operation.streaming',
  'operation.delegated',
  'operation.artifact_preview',
  'operation.artifact_completed',
  'operation.completed',
  'operation.failed',
  'operation.status_changed',
]);

// Messages we don't need to log (too noisy)
const SILENT_TYPES = new Set([
  'heartbeat',
  'stream',  // Too many deltas to log
  'terminal_output',  // Terminal output chunks can be frequent
  'document_processing_progress',
]);

const initialState = {
  socket: null,
  connectionState: 'disconnected' as const,
  reconnectAttempts: 0,
  maxReconnectAttempts: 10,
  reconnectDelay: 1000,
  lastMessage: null,
  messageQueue: [],
  listeners: new Map(),
  isConnected: false,
};

export const useWebSocketStore = create<WebSocketStore>()(
  subscribeWithSelector((set, get) => ({
    // Initial state
    ...initialState,
    
    connect: (url?: string) => {
      const baseUrl = url || WS_URL;
      const wsUrl = buildWsUrl(baseUrl);
      const { socket, connectionState, reconnectAttempts } = get();

      if (socket?.readyState === WebSocket.OPEN || connectionState === 'connecting') {
        return;
      }

      // FIXED: Use 'reconnecting' state if this is a retry
      const isReconnecting = reconnectAttempts > 0;
      set({
        connectionState: isReconnecting ? 'reconnecting' : 'connecting',
        isConnected: false,
      });

      try {
        const ws = new WebSocket(wsUrl);
        
        ws.onopen = () => {
          console.log('[WS] Connected');
          set({ 
            connectionState: 'connected', 
            reconnectAttempts: 0,
            socket: ws,
            isConnected: true,
          });
          
          get().processMessageQueue();
        };
        
        ws.onmessage = (event) => {
          try {
            const message = JSON.parse(event.data);
            get().handleMessage(message);
          } catch (error) {
            // ENHANCED: Better error logging for malformed JSON
            console.error('[WS] Failed to parse message:', error);
            console.error('[WS] Raw message data:', event.data?.substring(0, 200));
            
            // Don't crash - just log and continue
          }
        };
        
        ws.onerror = (error) => {
          console.error('[WS] Error:', error);
          set({ 
            connectionState: 'error',
            isConnected: false,
          });
        };
        
        ws.onclose = () => {
          console.log('[WS] Disconnected');
          set({ 
            connectionState: 'disconnected', 
            socket: null,
            isConnected: false,
          });
          
          const { reconnectAttempts, maxReconnectAttempts } = get();
          if (reconnectAttempts < maxReconnectAttempts) {
            get().scheduleReconnect();
          }
        };
        
        set({ socket: ws });
      } catch (error) {
        console.error('[WS] Connection failed:', error);
        set({ 
          connectionState: 'error',
          isConnected: false,
        });
      }
    },
    
    disconnect: () => {
      const { socket } = get();
      if (socket) {
        socket.close();
        set({ 
          socket: null, 
          connectionState: 'disconnected',
          isConnected: false,
        });
      }
    },
    
    send: async (message: any) => {
      const { socket, connectionState } = get();
      
      if (connectionState !== 'connected' || !socket) {
        set(state => ({ 
          messageQueue: [...state.messageQueue, message] 
        }));
        return;
      }
      
      try {
        const messageStr = JSON.stringify(message);
        socket.send(messageStr);
        
        if (message.type !== 'heartbeat' && message.method !== 'memory.get_recent') {
          console.log('[WS] Sent:', message.type, message.method || '');
        }
      } catch (error) {
        console.error('[WS] Failed to send message:', error);
        set(state => ({ 
          messageQueue: [...state.messageQueue, message] 
        }));
      }
    },
    
    // NEW: Alias for send() to match test expectations
    sendMessage: async (message: any) => {
      return get().send(message);
    },
    
    subscribe: (
      id: string, 
      callback: (message: WebSocketMessage) => void,
      messageTypes?: string[]
    ) => {
      const { listeners } = get();
      listeners.set(id, { callback, messageTypes });
      
      if (messageTypes) {
        console.log(`[WS] Subscribed: ${id} → [${messageTypes.join(', ')}]`);
      } else {
        console.log(`[WS] Subscribed: ${id} → [all messages]`);
      }
      
      return () => {
        const { listeners } = get();
        listeners.delete(id);
        console.log(`[WS] Unsubscribed: ${id}`);
      };
    },
    
    setConnectionState: (connectionState) => {
      set({ 
        connectionState,
        isConnected: connectionState === 'connected',
      });
    },
    
    handleMessage: (message: WebSocketMessage) => {
      // Validate message has required 'type' field
      if (!message || typeof message !== 'object' || !message.type) {
        console.warn('[WS] Received message missing required field "type":', message);
        return;
      }
      
      set({ lastMessage: message });
      
      // Smart logging
      const dataType = message.dataType || message.data?.type;
      const shouldLog = !SILENT_TYPES.has(message.type) && 
                        !SILENT_TYPES.has(dataType);
      
      if (shouldLog) {
        if (message.type === 'status') {
          console.log('[WS] Status:', message.message);
        } else if (message.type === 'data') {
          // Check if this is an operation event
          if (dataType?.startsWith('operation.')) {
            if (dataType === 'operation.started') {
              console.log('[WS] Operation started:', message.data?.operation_id);
            } else if (dataType === 'operation.completed') {
              console.log('[WS] Operation completed');
            } else if (dataType === 'operation.artifact_completed') {
              console.log('[WS] Artifact completed:', message.data?.artifact?.path);
            }
          } else if (dataType && KNOWN_DATA_TYPES.has(dataType)) {
            // Known data type - log briefly
            if (dataType === 'status') {
              console.log('[WS] Chat status:', message.data?.status);
            } else if (dataType === 'chat_complete') {
              console.log('[WS] Chat complete');
            } else if (dataType !== 'stream') {
              console.log(`[WS] Data: ${dataType}`);
            }
          } else if (dataType) {
            console.warn(`[WS] Unknown data type: ${dataType}`);
          }
        } else if (message.type === 'error') {
          console.error('[WS] Error:', message.message || message.error);
        } else if (!KNOWN_MESSAGE_TYPES.has(message.type)) {
          console.warn(`[WS] Unknown message type: ${message.type}`);
        }
      }
      
      // Notify filtered listeners
      const { listeners } = get();

      listeners.forEach((subscriber, id) => {
        const { callback, messageTypes } = subscriber;

        // If no filter specified, or message type matches filter
        // Also check nested data.type for wrapped events
        const dataType = message.data?.type || message.dataType;
        const shouldNotify = !messageTypes ||
                            messageTypes.includes(message.type) ||
                            (dataType && messageTypes.includes(dataType));

        if (shouldNotify) {
          try {
            callback(message);
          } catch (error) {
            console.error(`[WS] Listener error (${id}):`, error);
          }
        }
      });
    },
    
    processMessageQueue: () => {
      const { messageQueue } = get();
      
      if (messageQueue.length > 0) {
        console.log(`[WS] Processing ${messageQueue.length} queued messages`);
        
        messageQueue.forEach(msg => {
          get().send(msg);
        });
        
        set({ messageQueue: [] });
      }
    },
    
    scheduleReconnect: () => {
      const { reconnectAttempts, reconnectDelay, maxReconnectAttempts } = get();
      
      if (reconnectAttempts >= maxReconnectAttempts) {
        console.error('[WS] Max reconnection attempts reached');
        return;
      }
      
      const delay = Math.min(reconnectDelay * Math.pow(2, reconnectAttempts), 30000);
      console.log(`[WS] Reconnecting in ${delay}ms (attempt ${reconnectAttempts + 1}/${maxReconnectAttempts})`);
      
      set({ reconnectAttempts: reconnectAttempts + 1 });
      
      setTimeout(() => {
        const { connectionState } = get();
        if (connectionState === 'disconnected' || connectionState === 'error') {
          get().connect();
        }
      }, delay);
    },
    
    // NEW: Reset store to initial state (for testing and cleanup)
    reset: () => {
      const { socket } = get();
      if (socket) {
        socket.close();
      }
      set({
        ...initialState,
        listeners: new Map(), // Create new Map instance
      });
    },
  }))
);

// Auto-connect on store initialization (skip in test environment)
if (typeof import.meta.env.VITEST === 'undefined') {
  setTimeout(() => {
    useWebSocketStore.getState().connect();
  }, 100);
}
