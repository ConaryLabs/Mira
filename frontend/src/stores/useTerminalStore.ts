// src/stores/useTerminalStore.ts
// Terminal session state management with Zustand

import { create } from 'zustand';

export interface TerminalSession {
  id: string;
  projectId: string;
  workingDirectory: string;
  isActive: boolean;
  createdAt: string;
  cols: number;
  rows: number;
}

interface TerminalState {
  // Active sessions indexed by session_id
  sessions: Record<string, TerminalSession>;

  // Currently visible terminal session ID
  activeSessionId: string | null;

  // Terminal visibility
  isTerminalVisible: boolean;

  // Terminal panel size (percentage of viewport width)
  terminalWidth: number;

  // Actions
  addSession: (session: TerminalSession) => void;
  removeSession: (sessionId: string) => void;
  setActiveSession: (sessionId: string | null) => void;
  updateSession: (sessionId: string, updates: Partial<TerminalSession>) => void;
  toggleTerminalVisibility: () => void;
  showTerminal: () => void;
  hideTerminal: () => void;
  setTerminalWidth: (width: number) => void;
  clearSessions: () => void;
}

export const useTerminalStore = create<TerminalState>((set) => ({
  sessions: {},
  activeSessionId: null,
  isTerminalVisible: false,
  terminalWidth: 40, // Default 40% of viewport width

  addSession: (session) =>
    set((state) => ({
      sessions: {
        ...state.sessions,
        [session.id]: session,
      },
      activeSessionId: session.id,
      isTerminalVisible: true,
    })),

  removeSession: (sessionId) =>
    set((state) => {
      const { [sessionId]: removed, ...remaining } = state.sessions;
      const newActiveSessionId =
        state.activeSessionId === sessionId
          ? Object.keys(remaining)[0] || null
          : state.activeSessionId;

      return {
        sessions: remaining,
        activeSessionId: newActiveSessionId,
        isTerminalVisible: newActiveSessionId !== null,
      };
    }),

  setActiveSession: (sessionId) =>
    set({
      activeSessionId: sessionId,
      isTerminalVisible: sessionId !== null,
    }),

  updateSession: (sessionId, updates) =>
    set((state) => ({
      sessions: {
        ...state.sessions,
        [sessionId]: {
          ...state.sessions[sessionId],
          ...updates,
        },
      },
    })),

  toggleTerminalVisibility: () =>
    set((state) => ({
      isTerminalVisible: !state.isTerminalVisible,
    })),

  showTerminal: () =>
    set({
      isTerminalVisible: true,
    }),

  hideTerminal: () =>
    set({
      isTerminalVisible: false,
    }),

  setTerminalWidth: (width) =>
    set({
      terminalWidth: Math.max(20, Math.min(80, width)), // Clamp between 20% and 80%
    }),

  clearSessions: () =>
    set({
      sessions: {},
      activeSessionId: null,
      isTerminalVisible: false,
    }),
}));
