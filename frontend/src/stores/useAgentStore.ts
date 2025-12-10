// src/stores/useAgentStore.ts
// Store for managing background agents (Codex sessions)

import { create } from 'zustand';

export interface BackgroundAgent {
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

interface AgentState {
  // State
  agents: BackgroundAgent[];
  isPanelVisible: boolean;
  loading: boolean;
  selectedAgentId: string | null;

  // Actions
  setAgents: (agents: BackgroundAgent[]) => void;
  addAgent: (agent: BackgroundAgent) => void;
  updateAgent: (id: string, updates: Partial<BackgroundAgent>) => void;
  removeAgent: (id: string) => void;
  setLoading: (loading: boolean) => void;
  togglePanel: () => void;
  setPanelVisible: (visible: boolean) => void;
  selectAgent: (id: string | null) => void;

  // Derived
  runningAgents: () => BackgroundAgent[];
  completedAgents: () => BackgroundAgent[];
}

export const useAgentStore = create<AgentState>((set, get) => ({
  // Initial state
  agents: [],
  isPanelVisible: false,
  loading: false,
  selectedAgentId: null,

  // Actions
  setAgents: (agents) => set({ agents, loading: false }),

  addAgent: (agent) => set((state) => {
    // Check if agent already exists
    const existingIndex = state.agents.findIndex(a => a.id === agent.id);
    if (existingIndex >= 0) {
      // Update existing
      const updated = [...state.agents];
      updated[existingIndex] = { ...updated[existingIndex], ...agent };
      return { agents: updated };
    }
    // Add new
    return { agents: [...state.agents, agent] };
  }),

  updateAgent: (id, updates) => set((state) => ({
    agents: state.agents.map(a =>
      a.id === id ? { ...a, ...updates } : a
    )
  })),

  removeAgent: (id) => set((state) => ({
    agents: state.agents.filter(a => a.id !== id),
    selectedAgentId: state.selectedAgentId === id ? null : state.selectedAgentId
  })),

  setLoading: (loading) => set({ loading }),

  togglePanel: () => set((state) => ({ isPanelVisible: !state.isPanelVisible })),

  setPanelVisible: (visible) => set({ isPanelVisible: visible }),

  selectAgent: (id) => set({ selectedAgentId: id }),

  // Derived state as functions
  runningAgents: () => get().agents.filter(a => a.status === 'running'),

  completedAgents: () => get().agents.filter(a =>
    a.status === 'completed' || a.status === 'failed' || a.status === 'cancelled'
  ),
}));

// Helper hook for running agents count (for badge display)
export const useRunningAgentsCount = () => {
  return useAgentStore(state =>
    state.agents.filter(a => a.status === 'running').length
  );
};

// Format agent duration
export function formatAgentDuration(startedAt: number, completedAt?: number): string {
  const end = completedAt || Date.now() / 1000;
  const durationSecs = Math.floor(end - startedAt);

  if (durationSecs < 60) {
    return `${durationSecs}s`;
  } else if (durationSecs < 3600) {
    const mins = Math.floor(durationSecs / 60);
    const secs = durationSecs % 60;
    return `${mins}m ${secs}s`;
  } else {
    const hours = Math.floor(durationSecs / 3600);
    const mins = Math.floor((durationSecs % 3600) / 60);
    return `${hours}h ${mins}m`;
  }
}

// Format cost
export function formatCost(usd: number): string {
  if (usd < 0.01) {
    return `$${usd.toFixed(4)}`;
  }
  return `$${usd.toFixed(2)}`;
}
