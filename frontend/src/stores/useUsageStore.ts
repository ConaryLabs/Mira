// frontend/src/stores/useUsageStore.ts
// Store for tracking LLM usage and pricing information

import { create } from 'zustand';

export type PricingTier = 'standard' | 'large_context';
export type WarningLevel = 'none' | 'approaching' | 'near_threshold' | 'over_threshold';

export interface UsageInfo {
  operationId: string;
  tokensInput: number;
  tokensOutput: number;
  pricingTier: PricingTier;
  costUsd: number;
  fromCache: boolean;
  timestamp: number;
}

export interface ContextWarning {
  operationId: string;
  warningLevel: WarningLevel;
  message: string;
  tokensInput: number;
  threshold: number;
  timestamp: number;
}

interface UsageStore {
  // Current session usage tracking
  currentUsage: UsageInfo | null;
  sessionTotalCost: number;
  sessionTotalTokensInput: number;
  sessionTotalTokensOutput: number;
  cacheHits: number;
  cacheMisses: number;

  // Warnings
  currentWarning: ContextWarning | null;
  warningDismissed: boolean;

  // Actions
  updateUsage: (usage: UsageInfo) => void;
  setWarning: (warning: ContextWarning) => void;
  dismissWarning: () => void;
  resetSession: () => void;
}

export const useUsageStore = create<UsageStore>((set) => ({
  currentUsage: null,
  sessionTotalCost: 0,
  sessionTotalTokensInput: 0,
  sessionTotalTokensOutput: 0,
  cacheHits: 0,
  cacheMisses: 0,
  currentWarning: null,
  warningDismissed: false,

  updateUsage: (usage: UsageInfo) => {
    set((state) => ({
      currentUsage: usage,
      sessionTotalCost: state.sessionTotalCost + usage.costUsd,
      sessionTotalTokensInput: state.sessionTotalTokensInput + usage.tokensInput,
      sessionTotalTokensOutput: state.sessionTotalTokensOutput + usage.tokensOutput,
      cacheHits: usage.fromCache ? state.cacheHits + 1 : state.cacheHits,
      cacheMisses: usage.fromCache ? state.cacheMisses : state.cacheMisses + 1,
    }));
  },

  setWarning: (warning: ContextWarning) => {
    set({ currentWarning: warning, warningDismissed: false });
  },

  dismissWarning: () => {
    set({ warningDismissed: true });
  },

  resetSession: () => {
    set({
      currentUsage: null,
      sessionTotalCost: 0,
      sessionTotalTokensInput: 0,
      sessionTotalTokensOutput: 0,
      cacheHits: 0,
      cacheMisses: 0,
      currentWarning: null,
      warningDismissed: false,
    });
  },
}));
