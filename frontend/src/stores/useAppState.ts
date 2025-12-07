// src/stores/useAppState.ts
// FIXED: Less strict artifact validation - allows empty content, only rejects dangerous paths

import { create } from 'zustand';
import { persist, createJSONStorage } from 'zustand/middleware';
import type { Project } from '../types';
import type { Artifact } from './useChatStore';

export interface Toast {
  id: string;
  type: 'info' | 'success' | 'warning' | 'error';
  message: string;
  duration?: number;
  timestamp: number;
}

export type SystemAccessMode = 'project' | 'home' | 'system';

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
  
  // Artifacts (unified type from useChatStore)
  artifacts: Artifact[];
  activeArtifactId: string | null;
  appliedFiles: Set<string>;
  
  // Connection State (for error handling tests)
  isReconnecting: boolean;
  reconnectAttempts: number;
  connectionStatus: string;
  connectionError: string | null;
  
  // Rate Limiting
  canSendMessage: boolean;
  rateLimitUntil: number | null;

  // Toast Notifications
  toasts: Toast[];

  // System Access Mode (temporary elevated filesystem access)
  systemAccessMode: SystemAccessMode;
  
  // Actions - UI
  setShowArtifacts: (show: boolean) => void;
  setShowFileExplorer: (show: boolean) => void;
  
  // Actions - Projects
  setCurrentProject: (project: Project | null) => void;
  addProject: (project: Project) => void;
  setProjects: (projects: Project[] | ((prev: Project[]) => Project[])) => void;
  
  // Actions - Git
  addModifiedFile: (file: string) => void;
  removeModifiedFile: (file: string) => void;
  clearModifiedFiles: () => void;
  
  // Actions - Artifacts
  addArtifact: (artifact: Artifact) => void;
  setActiveArtifact: (id: string | null) => void;
  updateArtifact: (id: string, updates: Partial<Artifact>) => void;
  removeArtifact: (id: string) => void;
  markArtifactApplied: (id: string) => void;
  markArtifactUnapplied: (id: string) => void;
  isArtifactApplied: (id: string) => boolean;
  
  // Actions - Connection
  setReconnecting: (reconnecting: boolean) => void;
  setReconnectAttempts: (attempts: number) => void;
  setConnectionStatus: (status: string) => void;
  setConnectionError: (error: string | null) => void;
  
  // Actions - Rate Limiting
  setCanSendMessage: (can: boolean) => void;
  setRateLimitUntil: (timestamp: number | null) => void;
  
  // Actions - Toasts
  addToast: (toast: Omit<Toast, 'id' | 'timestamp'>) => void;
  removeToast: (id: string) => void;
  clearToasts: () => void;

  // Actions - System Access
  setSystemAccessMode: (mode: SystemAccessMode) => void;

  // Actions - Reset
  reset: () => void;
}

// Validate artifact path to prevent directory traversal
function isValidArtifactPath(path: string): boolean {
  if (!path || typeof path !== 'string') return false;
  
  // Reject paths with directory traversal attempts
  if (path.includes('..')) {
    console.warn('[AppState] Invalid artifact path (directory traversal):', path);
    return false;
  }
  
  // Reject absolute paths to system directories
  if (path.startsWith('/etc') || path.startsWith('/usr') || path.startsWith('/var')) {
    console.warn('[AppState] Invalid artifact path (system directory):', path);
    return false;
  }
  
  return true;
}

// Validate artifact has minimum required fields
// FIXED: Less strict - allows empty content, only requires id and safe path
function isValidArtifact(artifact: any): artifact is Artifact {
  if (!artifact || typeof artifact !== 'object') {
    console.warn('[AppState] Invalid artifact: not an object');
    return false;
  }
  
  if (!artifact.id || typeof artifact.id !== 'string') {
    console.warn('[AppState] Invalid artifact: missing or invalid id');
    return false;
  }
  
  if (!artifact.path || typeof artifact.path !== 'string') {
    console.warn('[AppState] Invalid artifact: missing or invalid path');
    return false;
  }
  
  // FIXED: Allow missing or empty content (test artifacts may not have content yet)
  // Only validate that IF content exists, it's a string
  if (artifact.content !== undefined && typeof artifact.content !== 'string') {
    console.warn('[AppState] Invalid artifact: content must be string if provided');
    return false;
  }
  
  if (!isValidArtifactPath(artifact.path)) {
    return false;
  }
  
  return true;
}

const initialState = {
  // UI State
  showArtifacts: false,
  showFileExplorer: false,
  quickOpenVisible: false,
  
  // Project State
  currentProject: null,
  projects: [],
  
  // Git State
  modifiedFiles: [],
  currentBranch: 'main',
  
  // Artifacts
  artifacts: [],
  activeArtifactId: null,
  appliedFiles: new Set<string>(),
  
  // Connection State
  isReconnecting: false,
  reconnectAttempts: 0,
  connectionStatus: '',
  connectionError: null,
  
  // Rate Limiting
  canSendMessage: true,
  rateLimitUntil: null,

  // Toasts
  toasts: [],

  // System Access (defaults to project-only, resets on page reload)
  systemAccessMode: 'project' as SystemAccessMode,
};

export const useAppState = create<AppState>()(
  persist(
    (set, get) => ({
      ...initialState,

      // ===== UI Actions =====
      setShowArtifacts: (show) => set({ showArtifacts: show }),
      setShowFileExplorer: (show) => set({ showFileExplorer: show }),
      
      // ===== Project Actions =====
      setCurrentProject: (project) => {
        set({ currentProject: project });
        if (project) {
          set({ modifiedFiles: [], currentBranch: 'main' });
        }
      },
      
      addProject: (project) => set((state) => ({
        projects: [...state.projects, project]
      })),
      
      setProjects: (projectsOrUpdater) => {
        set((state) => {
          const newProjects = typeof projectsOrUpdater === 'function' 
            ? projectsOrUpdater(state.projects)
            : projectsOrUpdater;
          
          const updatedCurrentProject = state.currentProject 
            ? newProjects.find(p => p.id === state.currentProject?.id) || state.currentProject
            : null;
          
          return {
            projects: newProjects,
            currentProject: updatedCurrentProject
          };
        });
      },
      
      // ===== Git Actions =====
      addModifiedFile: (file) => set((state) => ({
        modifiedFiles: state.modifiedFiles.includes(file) 
          ? state.modifiedFiles 
          : [...state.modifiedFiles, file]
      })),
      
      removeModifiedFile: (file) => set((state) => ({
        modifiedFiles: state.modifiedFiles.filter(f => f !== file)
      })),
      
      clearModifiedFiles: () => set({ modifiedFiles: [] }),
      
      // ===== Artifact Actions =====
      addArtifact: (artifact) => {
        // FIXED: Validate but continue with warning if minor issues
        if (!isValidArtifact(artifact)) {
          console.error('[AppState] Rejecting invalid artifact:', artifact);
          return;
        }
        
        set((state) => {
          const idx = state.artifacts.findIndex(a => 
            a.id === artifact.id || (!!artifact.path && a.path === artifact.path)
          );

          if (idx !== -1) {
            const updated = [...state.artifacts];
            updated[idx] = { ...updated[idx], ...artifact, id: updated[idx].id };
            return {
              artifacts: updated,
              activeArtifactId: updated[idx].id,
              showArtifacts: true,
            };
          }

          return {
            artifacts: [...state.artifacts, artifact],
            activeArtifactId: artifact.id,
            showArtifacts: true,
          };
        });
      },
      
      setActiveArtifact: (id) => set({ activeArtifactId: id }),
      
      updateArtifact: (id, updates) => set((state) => ({
        artifacts: state.artifacts.map(a => 
          a.id === id || (!!updates.path && a.path === updates.path)
            ? { ...a, ...updates }
            : a
        )
      })),
      
      removeArtifact: (id) => set((state) => {
        const newArtifacts = state.artifacts.filter(a => a.id !== id);
        return {
          artifacts: newArtifacts,
          activeArtifactId: state.activeArtifactId === id 
            ? (newArtifacts[0]?.id || null)
            : state.activeArtifactId,
          showArtifacts: newArtifacts.length > 0
        };
      }),
      
      markArtifactApplied: (id) => set((state) => ({
        appliedFiles: new Set(state.appliedFiles).add(id)
      })),
      
      markArtifactUnapplied: (id) => set((state) => {
        const newApplied = new Set(state.appliedFiles);
        newApplied.delete(id);
        return { appliedFiles: newApplied };
      }),
      
      isArtifactApplied: (id) => {
        return get().appliedFiles.has(id);
      },
      
      // ===== Connection Actions =====
      setReconnecting: (reconnecting) => set({ isReconnecting: reconnecting }),
      
      setReconnectAttempts: (attempts) => set({ reconnectAttempts: attempts }),
      
      setConnectionStatus: (status) => set({ connectionStatus: status }),
      
      setConnectionError: (error) => set({ connectionError: error }),
      
      // ===== Rate Limiting Actions =====
      setCanSendMessage: (can) => set({ canSendMessage: can }),
      
      setRateLimitUntil: (timestamp) => {
        set({ 
          rateLimitUntil: timestamp,
          canSendMessage: timestamp ? Date.now() >= timestamp : true,
        });
        
        // Auto-enable after rate limit expires
        if (timestamp && timestamp > Date.now()) {
          const delay = timestamp - Date.now();
          setTimeout(() => {
            set({ canSendMessage: true, rateLimitUntil: null });
          }, delay);
        }
      },
      
      // ===== Toast Actions =====
      addToast: (toast) => {
        const newToast: Toast = {
          ...toast,
          id: `toast-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`,
          timestamp: Date.now(),
        };
        
        set((state) => ({
          toasts: [...state.toasts, newToast]
        }));
        
        // Auto-remove after duration
        const duration = toast.duration || 5000;
        setTimeout(() => {
          get().removeToast(newToast.id);
        }, duration);
      },
      
      removeToast: (id) => set((state) => ({
        toasts: state.toasts.filter(t => t.id !== id)
      })),
      
      clearToasts: () => set({ toasts: [] }),

      // ===== System Access Actions =====
      setSystemAccessMode: (mode) => set({ systemAccessMode: mode }),

      // ===== Reset Action =====
      reset: () => {
        set({
          ...initialState,
          appliedFiles: new Set<string>(),
          toasts: [],
        });
      },
    }),
    {
      name: 'mira-app-state',
      storage: createJSONStorage(() => localStorage, {
        replacer: (key, value) => {
          if (value instanceof Set) {
            return Array.from(value);
          }
          return value;
        },
        reviver: (key, value) => {
          if (key === 'appliedFiles' && Array.isArray(value)) {
            return new Set(value);
          }
          return value;
        },
      }),
      partialize: (state) => ({
        currentProject: state.currentProject,
        projects: state.projects,
        artifacts: state.artifacts,
        activeArtifactId: state.activeArtifactId,
        appliedFiles: state.appliedFiles,
      }),
    }
  )
);

// Convenience hooks for specific parts of state
export const useProjectState = () => {
  const { currentProject, projects, modifiedFiles, currentBranch } = useAppState();
  return { currentProject, projects, modifiedFiles, currentBranch };
};

export const useArtifactState = () => {
  const { 
    artifacts, 
    activeArtifactId, 
    showArtifacts,
    appliedFiles,
    addArtifact,
    setActiveArtifact,
    updateArtifact,
    removeArtifact,
    markArtifactApplied,
    markArtifactUnapplied,
    isArtifactApplied,
  } = useAppState();
  
  const activeArtifact = artifacts.find(a => a.id === activeArtifactId) || null;
  
  return {
    artifacts,
    activeArtifact,
    showArtifacts,
    appliedFiles,
    addArtifact,
    setActiveArtifact,
    updateArtifact,
    removeArtifact,
    markArtifactApplied,
    markArtifactUnapplied,
    isArtifactApplied,
  };
};
