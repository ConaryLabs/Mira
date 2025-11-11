// src/stores/useUIStore.ts
// Transient UI state - input, modals, and tab navigation
// FIXED: Removed duplicate isWaitingForResponse (belongs in useChatStore)

import { create } from 'zustand';

type Tab = 'chat' | 'projects';

interface UIState {
  // Input state
  inputContent: string;
  
  // Modal state
  activeModal: string | null;
  
  // Tab navigation
  activeTab: Tab;
  
  // Actions
  setInputContent: (content: string) => void;
  clearInput: () => void;
  openModal: (id: string) => void;
  closeModal: () => void;
  setActiveTab: (tab: Tab) => void;
}

export const useUIStore = create<UIState>((set) => ({
  // Initial state
  inputContent: '',
  activeModal: null,
  activeTab: 'chat',
  
  // Actions
  setInputContent: (content) => set({ inputContent: content }),
  clearInput: () => set({ inputContent: '' }),
  openModal: (id) => set({ activeModal: id }),
  closeModal: () => set({ activeModal: null }),
  setActiveTab: (tab) => set({ activeTab: tab }),
}));

// Optimized selector hooks - components use these to avoid re-renders
export const useInputContent = () => useUIStore(state => state.inputContent);
export const useActiveModal = () => useUIStore(state => state.activeModal);
export const useActiveTab = () => useUIStore(state => state.activeTab);
