// Settings store for Mira Studio
// Uses Svelte writable stores with localStorage persistence

import { writable, get } from 'svelte/store';
import { browser } from '$app/environment';

export type ReasoningEffort = 'auto' | 'none' | 'low' | 'medium' | 'high' | 'xhigh';
// Note: Backend is now DeepSeek-only. This type kept for localStorage compatibility.
export type ModelProvider = 'deepseek';

export interface ProjectInfo {
  path: string;
  name: string;
  pinned: boolean;
  lastActivity?: number;  // timestamp
}

export interface Settings {
  reasoningEffort: ReasoningEffort;
  modelProvider: ModelProvider;
  projectPath: string;
  projectHistory: string[];
  projects: ProjectInfo[];  // Rich project data
  sidebarCollapsed: boolean;
}

const STORAGE_KEY = 'mira-settings';
const MAX_PROJECT_HISTORY = 10;

function extractProjectName(path: string): string {
  return path.split('/').filter(Boolean).pop() || path;
}

function getInitialSettings(): Settings {
  const defaults: Settings = {
    reasoningEffort: 'auto',
    modelProvider: 'deepseek',
    projectPath: '/home/peter/Mira',
    projectHistory: [],
    projects: [],
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
          projects: s.projects.filter((p) => p.path !== path),
        };
        persist(newSettings);
        return newSettings;
      });
    },
    addProject: (path: string) => {
      update((s) => {
        // Check if already exists
        const existing = s.projects.find((p) => p.path === path);
        if (existing) {
          // Just update lastActivity
          const projects = s.projects.map((p) =>
            p.path === path ? { ...p, lastActivity: Date.now() } : p
          );
          const newSettings = { ...s, projectPath: path, projects };
          persist(newSettings);
          return newSettings;
        }
        // Add new project
        const newProject: ProjectInfo = {
          path,
          name: extractProjectName(path),
          pinned: false,
          lastActivity: Date.now(),
        };
        const history = s.projectHistory.filter((p) => p !== path);
        history.unshift(path);
        const newSettings = {
          ...s,
          projectPath: path,
          projectHistory: history.slice(0, MAX_PROJECT_HISTORY),
          projects: [...s.projects, newProject],
        };
        persist(newSettings);
        return newSettings;
      });
    },
    togglePinned: (path: string) => {
      update((s) => {
        const projects = s.projects.map((p) =>
          p.path === path ? { ...p, pinned: !p.pinned } : p
        );
        const newSettings = { ...s, projects };
        persist(newSettings);
        return newSettings;
      });
    },
    updateProjectActivity: (path: string) => {
      update((s) => {
        const projects = s.projects.map((p) =>
          p.path === path ? { ...p, lastActivity: Date.now() } : p
        );
        const newSettings = { ...s, projects };
        persist(newSettings);
        return newSettings;
      });
    },
    getProjectInfo: (path: string): ProjectInfo | undefined => {
      return get(settings).projects.find((p) => p.path === path);
    },
    getSortedProjects: (): ProjectInfo[] => {
      const s = get(settings);
      // Merge history with projects (for migration)
      const allPaths = new Set([...s.projects.map((p) => p.path), ...s.projectHistory]);
      const projects: ProjectInfo[] = [];
      for (const path of allPaths) {
        const existing = s.projects.find((p) => p.path === path);
        if (existing) {
          projects.push(existing);
        } else {
          // Create from history
          projects.push({
            path,
            name: extractProjectName(path),
            pinned: false,
          });
        }
      }
      // Sort: pinned first, then by lastActivity
      return projects.sort((a, b) => {
        if (a.pinned !== b.pinned) return a.pinned ? -1 : 1;
        return (b.lastActivity || 0) - (a.lastActivity || 0);
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

// Model options - DeepSeek is now the only option (council tool provides access to other models)
export const modelProviderOptions: { value: ModelProvider; label: string; description: string }[] = [
  { value: 'deepseek', label: 'DeepSeek V3.2', description: 'Primary model (use council tool for GPT/Opus/Gemini)' },
];
