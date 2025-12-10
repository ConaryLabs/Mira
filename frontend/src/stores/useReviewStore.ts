// src/stores/useReviewStore.ts
// Store for code review mode state

import { create } from 'zustand';

export type ReviewTarget = 'uncommitted' | 'staged' | 'branch' | 'commit';

export interface ReviewState {
  // State
  isReviewMode: boolean;
  isPanelVisible: boolean;
  loading: boolean;
  diff: string | null;
  reviewTarget: ReviewTarget;
  baseBranch: string;
  commitHash: string;
  reviewResult: string | null;
  reviewing: boolean;

  // Stats
  additions: number;
  deletions: number;
  filesChanged: string[];

  // Actions
  setReviewMode: (enabled: boolean) => void;
  setPanelVisible: (visible: boolean) => void;
  togglePanel: () => void;
  setLoading: (loading: boolean) => void;
  setDiff: (diff: string | null) => void;
  setReviewTarget: (target: ReviewTarget) => void;
  setBaseBranch: (branch: string) => void;
  setCommitHash: (hash: string) => void;
  setReviewResult: (result: string | null) => void;
  setReviewing: (reviewing: boolean) => void;
  setStats: (additions: number, deletions: number, files: string[]) => void;
  reset: () => void;
}

const initialState = {
  isReviewMode: false,
  isPanelVisible: false,
  loading: false,
  diff: null,
  reviewTarget: 'uncommitted' as ReviewTarget,
  baseBranch: 'main',
  commitHash: '',
  reviewResult: null,
  reviewing: false,
  additions: 0,
  deletions: 0,
  filesChanged: [],
};

export const useReviewStore = create<ReviewState>((set) => ({
  ...initialState,

  setReviewMode: (enabled) => set({ isReviewMode: enabled }),

  setPanelVisible: (visible) => set({ isPanelVisible: visible }),

  togglePanel: () => set((state) => ({ isPanelVisible: !state.isPanelVisible })),

  setLoading: (loading) => set({ loading }),

  setDiff: (diff) => {
    if (diff) {
      // Parse stats from diff
      const lines = diff.split('\n');
      let additions = 0;
      let deletions = 0;
      const files = new Set<string>();

      for (const line of lines) {
        if (line.startsWith('+') && !line.startsWith('+++')) {
          additions++;
        } else if (line.startsWith('-') && !line.startsWith('---')) {
          deletions++;
        } else if (line.startsWith('diff --git')) {
          // Extract filename from "diff --git a/path b/path"
          const match = line.match(/diff --git a\/(.+) b\//);
          if (match) {
            files.add(match[1]);
          }
        }
      }

      set({
        diff,
        additions,
        deletions,
        filesChanged: Array.from(files),
      });
    } else {
      set({ diff: null, additions: 0, deletions: 0, filesChanged: [] });
    }
  },

  setReviewTarget: (target) => set({ reviewTarget: target }),

  setBaseBranch: (branch) => set({ baseBranch: branch }),

  setCommitHash: (hash) => set({ commitHash: hash }),

  setReviewResult: (result) => set({ reviewResult: result }),

  setReviewing: (reviewing) => set({ reviewing }),

  setStats: (additions, deletions, files) => set({
    additions,
    deletions,
    filesChanged: files,
  }),

  reset: () => set(initialState),
}));

// Parse diff stats helper (exported for use elsewhere)
export function parseDiffStats(diff: string): { additions: number; deletions: number; files: string[] } {
  const lines = diff.split('\n');
  let additions = 0;
  let deletions = 0;
  const files = new Set<string>();

  for (const line of lines) {
    if (line.startsWith('+') && !line.startsWith('+++')) {
      additions++;
    } else if (line.startsWith('-') && !line.startsWith('---')) {
      deletions++;
    } else if (line.startsWith('diff --git')) {
      const match = line.match(/diff --git a\/(.+) b\//);
      if (match) {
        files.add(match[1]);
      }
    }
  }

  return { additions, deletions, files: Array.from(files) };
}
