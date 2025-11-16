// src/stores/useChatStore.ts
// ENHANCED: Added error handling, reset(), setStreaming(), incomplete message tracking

import { create } from 'zustand';
import { persist } from 'zustand/middleware';

export type ArtifactStatus = 'draft' | 'saved' | 'applied';

export interface Artifact {
  id: string;
  title?: string;
  path: string;
  content: string;
  language?: string;
  changeType?: 'primary' | 'import' | 'type' | 'cascade';
  timestamp?: number;
  status?: ArtifactStatus;
  origin?: 'llm' | 'user';
}

export type TaskStatus = 'pending' | 'running' | 'completed' | 'failed';

export interface Plan {
  plan_text: string;
  reasoning_tokens: number;
  timestamp: number;
}

export interface Task {
  task_id: string;
  sequence: number;
  description: string;
  active_form: string;
  status: TaskStatus;
  error?: string;
  timestamp: number;
}

export interface ToolExecution {
  toolName: string;
  toolType: string;
  summary: string;
  success: boolean;
  details?: any;
  timestamp: number;
}

export interface ChatMessage {
  id: string;
  role: 'user' | 'assistant' | 'system' | 'error'; // ADDED: error role
  content: string;
  artifacts?: Artifact[];
  timestamp: number;
  isStreaming?: boolean;
  isIncomplete?: boolean; // NEW: Mark messages that were cut off
  operationId?: string; // NEW: Track which operation this belongs to
  plan?: Plan; // NEW: Plan generated for this operation
  tasks?: Task[]; // NEW: Tasks for this operation
  toolExecutions?: ToolExecution[]; // NEW: Tool executions for this operation
  metadata?: {
    session_id?: string;
    project_id?: string;
    file_path?: string;
    [key: string]: any;
  };
}

interface ChatStore {
  messages: ChatMessage[];
  currentSessionId: string;
  isWaitingForResponse: boolean;
  isStreaming: boolean;
  streamingContent: string;
  streamingMessageId: string | null;
  currentStreamingMessageId: string | null; // Alias for compatibility

  addMessage: (message: ChatMessage) => void;
  updateMessage: (id: string, updates: Partial<ChatMessage>) => void;
  setMessages: (messages: ChatMessage[]) => void;
  clearMessages: () => void;
  setSessionId: (id: string) => void;
  setWaitingForResponse: (waiting: boolean) => void;
  startStreaming: () => void;
  appendStreamContent: (content: string) => void;
  endStreaming: () => void;
  clearStreaming: () => void;
  setStreaming: (streaming: boolean) => void; // NEW: Direct streaming control
  reset: () => void; // NEW: Reset store state

  // NEW: Planning mode and task tracking methods
  updateMessagePlan: (messageId: string, plan: Plan) => void;
  addMessageTask: (messageId: string, task: Task) => void;
  updateTaskStatus: (messageId: string, taskId: string, status: TaskStatus, error?: string) => void;
  setMessageOperationId: (messageId: string, operationId: string) => void;

  // NEW: Tool execution tracking methods
  addToolExecution: (messageId: string, execution: ToolExecution) => void;
}

const initialState = {
  messages: [],
  currentSessionId: 'peter-eternal',
  isWaitingForResponse: false,
  isStreaming: false,
  streamingContent: '',
  streamingMessageId: null,
  currentStreamingMessageId: null,
};

export const useChatStore = create<ChatStore>()(
  persist(
    (set, get) => ({
      ...initialState,
      
      addMessage: (message) => {
        set(state => ({
          messages: [...state.messages, message],
          isWaitingForResponse: message.role === 'assistant' ? false : state.isWaitingForResponse
        }));
      },
      
      updateMessage: (id, updates) => {
        set(state => ({
          messages: state.messages.map(msg => 
            msg.id === id ? { ...msg, ...updates } : msg
          )
        }));
      },
      
      setMessages: (messages) => set({ messages }),
      
      clearMessages: () => set({ messages: [] }),
      
      setSessionId: (id) => set({ currentSessionId: id }),
      
      setWaitingForResponse: (waiting) => set({ isWaitingForResponse: waiting }),
      
      startStreaming: () => {
        const streamId = `stream-${Date.now()}`;
        set({ 
          isStreaming: true, 
          streamingContent: '', 
          streamingMessageId: streamId,
          currentStreamingMessageId: streamId,
        });
      },
      
      appendStreamContent: (content) => set(state => ({ 
        streamingContent: state.streamingContent + content 
      })),
      
      endStreaming: () => {
        const { streamingContent, streamingMessageId } = get();
        if (streamingContent && streamingMessageId) {
          set(state => ({
            messages: [...state.messages, {
              id: streamingMessageId,
              role: 'assistant',
              content: streamingContent,
              timestamp: Date.now(),
            }],
            isStreaming: false,
            streamingContent: '',
            streamingMessageId: null,
            currentStreamingMessageId: null,
            isWaitingForResponse: false,
          }));
        } else {
          set({ 
            isStreaming: false, 
            streamingContent: '',
            streamingMessageId: null,
            currentStreamingMessageId: null,
            isWaitingForResponse: false,
          });
        }
      },
      
      clearStreaming: () => set({ 
        isStreaming: false, 
        streamingContent: '', 
        streamingMessageId: null,
        currentStreamingMessageId: null,
      }),
      
      // NEW: Direct streaming control (for tests and manual control)
      setStreaming: (streaming: boolean) => {
        if (streaming && !get().isStreaming) {
          // Starting streaming
          get().startStreaming();
        } else if (!streaming && get().isStreaming) {
          // Stopping streaming without finalizing
          set({
            isStreaming: false,
            streamingContent: '',
            streamingMessageId: null,
            currentStreamingMessageId: null,
          });
        }
      },
      
      // NEW: Reset store to initial state (for testing and cleanup)
      reset: () => {
        set({
          ...initialState,
          messages: [], // Explicitly clear messages
        });
      },

      // NEW: Planning mode and task tracking implementations
      updateMessagePlan: (messageId, plan) => {
        set(state => ({
          messages: state.messages.map(msg =>
            msg.id === messageId ? { ...msg, plan } : msg
          )
        }));
      },

      addMessageTask: (messageId, task) => {
        set(state => ({
          messages: state.messages.map(msg => {
            if (msg.id === messageId) {
              const existingTasks = msg.tasks || [];
              // Check if task already exists (avoid duplicates)
              if (existingTasks.some(t => t.task_id === task.task_id)) {
                return msg;
              }
              // Insert task in correct sequence order
              const updatedTasks = [...existingTasks, task].sort((a, b) => a.sequence - b.sequence);
              return { ...msg, tasks: updatedTasks };
            }
            return msg;
          })
        }));
      },

      updateTaskStatus: (messageId, taskId, status, error) => {
        set(state => ({
          messages: state.messages.map(msg => {
            if (msg.id === messageId && msg.tasks) {
              return {
                ...msg,
                tasks: msg.tasks.map(task =>
                  task.task_id === taskId
                    ? { ...task, status, ...(error && { error }) }
                    : task
                )
              };
            }
            return msg;
          })
        }));
      },

      setMessageOperationId: (messageId, operationId) => {
        set(state => ({
          messages: state.messages.map(msg =>
            msg.id === messageId ? { ...msg, operationId } : msg
          )
        }));
      },

      addToolExecution: (messageId, execution) => {
        set(state => ({
          messages: state.messages.map(msg => {
            if (msg.id === messageId) {
              const existingExecutions = msg.toolExecutions || [];
              return { ...msg, toolExecutions: [...existingExecutions, execution] };
            }
            return msg;
          })
        }));
      },
    }),
    {
      name: 'mira-chat-storage',
      partialize: (state) => ({
        messages: state.messages,
        currentSessionId: state.currentSessionId,
      }),
    }
  )
);

export function useCurrentSession() {
  const messages = useChatStore(state => state.messages);
  const sessionId = useChatStore(state => state.currentSessionId);
  return { messages, sessionId };
}
