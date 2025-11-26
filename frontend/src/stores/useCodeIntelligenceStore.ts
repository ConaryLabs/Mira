// frontend/src/stores/useCodeIntelligenceStore.ts
// Store for code intelligence features: budget tracking, semantic search, co-change suggestions

import { create } from 'zustand';
import { subscribeWithSelector } from 'zustand/middleware';

// ===== Budget Types =====
export interface BudgetStatus {
  dailyUsagePercent: number;
  monthlyUsagePercent: number;
  dailySpentUsd: number;
  dailyLimitUsd: number;
  monthlySpentUsd: number;
  monthlyLimitUsd: number;
  dailyRemaining: number;
  monthlyRemaining: number;
  isCritical: boolean;
  isLow: boolean;
  lastUpdated: number;
}

// ===== Code Search Types =====
export interface CodeSearchResult {
  id: string;
  filePath: string;
  content: string;
  score: number;
  lineStart: number;
  lineEnd: number;
  language?: string;
  highlights?: string[];
}

export interface CodeSearchState {
  query: string;
  results: CodeSearchResult[];
  isSearching: boolean;
  error: string | null;
  lastSearchTime: number;
}

// ===== Co-Change Types =====
export interface CoChangeSuggestion {
  filePath: string;
  confidence: number;
  reason: string;
  coChangeCount: number;
  lastChanged: string;
}

// ===== Historical Fix Types =====
export interface HistoricalFix {
  id: string;
  errorPattern: string;
  fixDescription: string;
  filePath: string;
  similarity: number;
  commitHash?: string;
  author?: string;
  date: string;
}

// ===== Expertise Types =====
export interface AuthorExpertise {
  author: string;
  email: string;
  filesOwned: number;
  totalCommits: number;
  recentActivity: number;
  expertiseAreas: string[];
}

// ===== Build Error Types =====
export interface BuildError {
  id: string;
  errorType: string;
  message: string;
  filePath?: string;
  line?: number;
  column?: number;
  severity: 'error' | 'warning' | 'info';
  timestamp: number;
  suggestedFix?: string;
}

// ===== Store Interface =====
interface CodeIntelligenceStore {
  // Budget state
  budget: BudgetStatus | null;
  isBudgetLoading: boolean;
  budgetError: string | null;

  // Code search state
  codeSearch: CodeSearchState;

  // Co-change suggestions
  coChangeSuggestions: CoChangeSuggestion[];
  isLoadingCoChange: boolean;
  currentFile: string | null;

  // Historical fixes
  historicalFixes: HistoricalFix[];
  isLoadingFixes: boolean;
  currentError: string | null;

  // Author expertise
  expertise: AuthorExpertise[];
  isLoadingExpertise: boolean;

  // Build errors
  buildErrors: BuildError[];
  isLoadingBuildErrors: boolean;

  // Panel visibility
  isPanelVisible: boolean;
  activeTab: 'budget' | 'search' | 'cochange' | 'fixes' | 'expertise';

  // Actions - Budget
  setBudget: (budget: BudgetStatus) => void;
  setBudgetLoading: (loading: boolean) => void;
  setBudgetError: (error: string | null) => void;
  refreshBudget: () => void;

  // Actions - Code Search
  setSearchQuery: (query: string) => void;
  setSearchResults: (results: CodeSearchResult[]) => void;
  setSearching: (searching: boolean) => void;
  setSearchError: (error: string | null) => void;
  clearSearch: () => void;

  // Actions - Co-Change
  setCoChangeSuggestions: (suggestions: CoChangeSuggestion[]) => void;
  setLoadingCoChange: (loading: boolean) => void;
  setCurrentFile: (file: string | null) => void;

  // Actions - Historical Fixes
  setHistoricalFixes: (fixes: HistoricalFix[]) => void;
  setLoadingFixes: (loading: boolean) => void;
  setCurrentError: (error: string | null) => void;

  // Actions - Expertise
  setExpertise: (expertise: AuthorExpertise[]) => void;
  setLoadingExpertise: (loading: boolean) => void;

  // Actions - Build Errors
  setBuildErrors: (errors: BuildError[]) => void;
  addBuildError: (error: BuildError) => void;
  clearBuildErrors: () => void;
  setLoadingBuildErrors: (loading: boolean) => void;

  // Actions - Panel
  togglePanel: () => void;
  setActiveTab: (tab: CodeIntelligenceStore['activeTab']) => void;
  showPanel: () => void;
  hidePanel: () => void;

  // Actions - Reset
  reset: () => void;
}

const initialBudget: BudgetStatus = {
  dailyUsagePercent: 0,
  monthlyUsagePercent: 0,
  dailySpentUsd: 0,
  dailyLimitUsd: 5.0,
  monthlySpentUsd: 0,
  monthlyLimitUsd: 150.0,
  dailyRemaining: 5.0,
  monthlyRemaining: 150.0,
  isCritical: false,
  isLow: false,
  lastUpdated: 0,
};

const initialCodeSearch: CodeSearchState = {
  query: '',
  results: [],
  isSearching: false,
  error: null,
  lastSearchTime: 0,
};

const initialState = {
  budget: null,
  isBudgetLoading: false,
  budgetError: null,
  codeSearch: initialCodeSearch,
  coChangeSuggestions: [],
  isLoadingCoChange: false,
  currentFile: null,
  historicalFixes: [],
  isLoadingFixes: false,
  currentError: null,
  expertise: [],
  isLoadingExpertise: false,
  buildErrors: [],
  isLoadingBuildErrors: false,
  isPanelVisible: false,
  activeTab: 'budget' as const,
};

export const useCodeIntelligenceStore = create<CodeIntelligenceStore>()(
  subscribeWithSelector((set, get) => ({
    ...initialState,

    // ===== Budget Actions =====
    setBudget: (budget) => set({ budget, budgetError: null }),

    setBudgetLoading: (loading) => set({ isBudgetLoading: loading }),

    setBudgetError: (error) => set({ budgetError: error, isBudgetLoading: false }),

    refreshBudget: () => {
      // This will be called by the WebSocket handler when budget data arrives
      set({ isBudgetLoading: true });
    },

    // ===== Code Search Actions =====
    setSearchQuery: (query) => set((state) => ({
      codeSearch: { ...state.codeSearch, query },
    })),

    setSearchResults: (results) => set((state) => ({
      codeSearch: {
        ...state.codeSearch,
        results,
        isSearching: false,
        lastSearchTime: Date.now(),
      },
    })),

    setSearching: (searching) => set((state) => ({
      codeSearch: { ...state.codeSearch, isSearching: searching },
    })),

    setSearchError: (error) => set((state) => ({
      codeSearch: { ...state.codeSearch, error, isSearching: false },
    })),

    clearSearch: () => set({ codeSearch: initialCodeSearch }),

    // ===== Co-Change Actions =====
    setCoChangeSuggestions: (suggestions) => set({
      coChangeSuggestions: suggestions,
      isLoadingCoChange: false,
    }),

    setLoadingCoChange: (loading) => set({ isLoadingCoChange: loading }),

    setCurrentFile: (file) => set({ currentFile: file }),

    // ===== Historical Fixes Actions =====
    setHistoricalFixes: (fixes) => set({
      historicalFixes: fixes,
      isLoadingFixes: false,
    }),

    setLoadingFixes: (loading) => set({ isLoadingFixes: loading }),

    setCurrentError: (error) => set({ currentError: error }),

    // ===== Expertise Actions =====
    setExpertise: (expertise) => set({
      expertise,
      isLoadingExpertise: false,
    }),

    setLoadingExpertise: (loading) => set({ isLoadingExpertise: loading }),

    // ===== Build Error Actions =====
    setBuildErrors: (errors) => set({
      buildErrors: errors,
      isLoadingBuildErrors: false,
    }),

    addBuildError: (error) => set((state) => ({
      buildErrors: [...state.buildErrors, error],
    })),

    clearBuildErrors: () => set({ buildErrors: [] }),

    setLoadingBuildErrors: (loading) => set({ isLoadingBuildErrors: loading }),

    // ===== Panel Actions =====
    togglePanel: () => set((state) => ({ isPanelVisible: !state.isPanelVisible })),

    setActiveTab: (tab) => set({ activeTab: tab }),

    showPanel: () => set({ isPanelVisible: true }),

    hidePanel: () => set({ isPanelVisible: false }),

    // ===== Reset =====
    reset: () => set(initialState),
  }))
);

// ===== Convenience Hooks =====
export const useBudgetStatus = () => {
  const budget = useCodeIntelligenceStore((state) => state.budget);
  const isLoading = useCodeIntelligenceStore((state) => state.isBudgetLoading);
  const error = useCodeIntelligenceStore((state) => state.budgetError);
  return { budget, isLoading, error };
};

export const useCodeSearch = () => {
  const codeSearch = useCodeIntelligenceStore((state) => state.codeSearch);
  const setQuery = useCodeIntelligenceStore((state) => state.setSearchQuery);
  const clearSearch = useCodeIntelligenceStore((state) => state.clearSearch);
  return { ...codeSearch, setQuery, clearSearch };
};

export const useCoChangeSuggestions = () => {
  const suggestions = useCodeIntelligenceStore((state) => state.coChangeSuggestions);
  const isLoading = useCodeIntelligenceStore((state) => state.isLoadingCoChange);
  const currentFile = useCodeIntelligenceStore((state) => state.currentFile);
  return { suggestions, isLoading, currentFile };
};

export const useBuildErrors = () => {
  const errors = useCodeIntelligenceStore((state) => state.buildErrors);
  const isLoading = useCodeIntelligenceStore((state) => state.isLoadingBuildErrors);
  const clearErrors = useCodeIntelligenceStore((state) => state.clearBuildErrors);
  return { errors, isLoading, clearErrors };
};
