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

export interface ChatMessage {
  id: string;
  role: 'user' | 'assistant' | 'system' | 'error'; // ADDED: error role
  content: string;
  artifacts?: Artifact[];
  timestamp: number;
  isStreaming?: boolean;
  isIncomplete?: boolean; // NEW: Mark messages that were cut off
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
