// Settings store for Mira Studio
// Uses Svelte writable stores with localStorage persistence

import { writable, get } from 'svelte/store';
import { browser } from '$app/environment';

export type ReasoningEffort = 'auto' | 'none' | 'low' | 'medium' | 'high' | 'xhigh';
export type ModelProvider = 'gpt' | 'deepseek';

export interface Settings {
  reasoningEffort: ReasoningEffort;
  modelProvider: ModelProvider;
  projectPath: string;
  projectHistory: string[];
  sidebarCollapsed: boolean;
}

const STORAGE_KEY = 'mira-settings';
const MAX_PROJECT_HISTORY = 10;

function getInitialSettings(): Settings {
  const defaults: Settings = {
    reasoningEffort: 'auto',
    modelProvider: 'gpt',
    projectPath: '/home/peter/Mira',
    projectHistory: [],
    sidebarCollapsed: false,
  };

  if (!browser) return defaults;

  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored) {
      const parsed = JSON.parse(stored);
      return { ...defaults, ...parsed };
    }
  } catch {
    // Ignore parse errors
  }

  // Migrate old project path if exists
  const oldProject = localStorage.getItem('mira-project-path');
  if (oldProject) {
    defaults.projectPath = oldProject;
    defaults.projectHistory = [oldProject];
  }

  return defaults;
}

function createSettingsStore() {
  const { subscribe, set, update } = writable<Settings>(getInitialSettings());

  function persist(settings: Settings) {
    if (browser) {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
    }
  }

  return {
    subscribe,
    setReasoningEffort: (effort: ReasoningEffort) => {
      update((s) => {
        const newSettings = { ...s, reasoningEffort: effort };
        persist(newSettings);
        return newSettings;
      });
    },
    setModelProvider: (provider: ModelProvider) => {
      update((s) => {
        const newSettings = { ...s, modelProvider: provider };
        persist(newSettings);
        return newSettings;
      });
    },
    setProjectPath: (path: string) => {
      update((s) => {
        const history = s.projectHistory.filter((p) => p !== path);
        history.unshift(path);
        const newSettings = {
          ...s,
          projectPath: path,
          projectHistory: history.slice(0, MAX_PROJECT_HISTORY),
        };
        persist(newSettings);
        return newSettings;
      });
    },
    setSidebarCollapsed: (collapsed: boolean) => {
      update((s) => {
        const newSettings = { ...s, sidebarCollapsed: collapsed };
        persist(newSettings);
        return newSettings;
      });
    },
    removeFromHistory: (path: string) => {
      update((s) => {
        const newSettings = {
          ...s,
          projectHistory: s.projectHistory.filter((p) => p !== path),
        };
        persist(newSettings);
        return newSettings;
      });
    },
  };
}

export const settings = createSettingsStore();

// Helper for components - returns store values directly
export function useSettings() {
  return settings;
}

export const reasoningOptions: { value: ReasoningEffort; label: string; description: string }[] = [
  { value: 'auto', label: 'Auto', description: 'Let AI decide based on query complexity' },
  { value: 'none', label: 'None', description: 'Fastest - no extended thinking' },
  { value: 'low', label: 'Low', description: 'Quick queries and simple tasks' },
  { value: 'medium', label: 'Medium', description: 'Standard development tasks' },
  { value: 'high', label: 'High', description: 'Complex problems and debugging' },
  { value: 'xhigh', label: 'X-High', description: 'Critical analysis and architecture' },
];

export const modelProviderOptions: { value: ModelProvider; label: string; description: string }[] = [
  { value: 'gpt', label: 'GPT 5.2', description: 'Full capability with reasoning' },
  { value: 'deepseek', label: 'DeepSeek V3.2', description: 'Cost effective with reasoning' },
];
