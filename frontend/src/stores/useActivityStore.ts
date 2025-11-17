// frontend/src/stores/useActivityStore.ts
// Activity Panel state management - tracks current operation execution details

import { create } from 'zustand';
import { useChatStore, Plan, Task, ToolExecution } from './useChatStore';

export interface ActivityEntry {
  type: 'plan' | 'task' | 'tool';
  timestamp: number;
  data: Plan | Task | ToolExecution;
}

interface ActivityStore {
  // Panel visibility
  isPanelVisible: boolean;
  panelWidth: number;

  // Current operation tracking
  currentOperationId: string | null;
  currentMessageId: string | null;

  // Panel controls
  togglePanel: () => void;
  showPanel: () => void;
  hidePanel: () => void;
  setPanelWidth: (width: number) => void;

  // Operation tracking
  setCurrentOperation: (operationId: string, messageId: string) => void;
  clearCurrentOperation: () => void;

  // Data accessors (get from useChatStore)
  getCurrentPlan: () => Plan | undefined;
  getCurrentTasks: () => Task[];
  getCurrentToolExecutions: () => ToolExecution[];
  getAllActivity: () => ActivityEntry[];
}

export const useActivityStore = create<ActivityStore>((set, get) => ({
  // Initial state
  isPanelVisible: true,
  panelWidth: 400,
  currentOperationId: null,
  currentMessageId: null,

  // Panel controls
  togglePanel: () => set(state => ({ isPanelVisible: !state.isPanelVisible })),
  showPanel: () => set({ isPanelVisible: true }),
  hidePanel: () => set({ isPanelVisible: false }),
  setPanelWidth: (width) => set({ panelWidth: Math.max(300, Math.min(800, width)) }),

  // Operation tracking
  setCurrentOperation: (operationId, messageId) => {
    set({ currentOperationId: operationId, currentMessageId: messageId });
  },

  clearCurrentOperation: () => {
    set({ currentOperationId: null, currentMessageId: null });
  },

  // Data accessors - pull from useChatStore
  getCurrentPlan: () => {
    const state = get();
    if (!state.currentMessageId) return undefined;

    const message = useChatStore.getState().messages.find(m => m.id === state.currentMessageId);
    return message?.plan;
  },

  getCurrentTasks: () => {
    const state = get();
    if (!state.currentMessageId) return [];

    const message = useChatStore.getState().messages.find(m => m.id === state.currentMessageId);
    return message?.tasks || [];
  },

  getCurrentToolExecutions: () => {
    const state = get();
    if (!state.currentMessageId) return [];

    const message = useChatStore.getState().messages.find(m => m.id === state.currentMessageId);
    return message?.toolExecutions || [];
  },

  getAllActivity: () => {
    const state = get();
    const activities: ActivityEntry[] = [];

    // Get plan
    const plan = state.getCurrentPlan();
    if (plan) {
      activities.push({
        type: 'plan',
        timestamp: plan.timestamp,
        data: plan,
      });
    }

    // Get tasks
    const tasks = state.getCurrentTasks();
    tasks.forEach(task => {
      activities.push({
        type: 'task',
        timestamp: task.timestamp,
        data: task,
      });
    });

    // Get tool executions
    const toolExecutions = state.getCurrentToolExecutions();
    toolExecutions.forEach(execution => {
      activities.push({
        type: 'tool',
        timestamp: execution.timestamp,
        data: execution,
      });
    });

    // Sort by timestamp (oldest first)
    return activities.sort((a, b) => a.timestamp - b.timestamp);
  },
}));
