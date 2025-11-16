// src/stores/useUIStore.ts
// Transient UI state - input and modals

import { create } from 'zustand';

interface UIState {
  // Input state
  inputContent: string;

  // Modal state
  activeModal: string | null;

  // Actions
  setInputContent: (content: string) => void;
  clearInput: () => void;
  openModal: (id: string) => void;
  closeModal: () => void;
}

export const useUIStore = create<UIState>((set) => ({
  // Initial state
  inputContent: '',
  activeModal: null,

  // Actions
  setInputContent: (content) => set({ inputContent: content }),
  clearInput: () => set({ inputContent: '' }),
  openModal: (id) => set({ activeModal: id }),
  closeModal: () => set({ activeModal: null }),
}));

// Optimized selector hooks - components use these to avoid re-renders
export const useInputContent = () => useUIStore(state => state.inputContent);
export const useActiveModal = () => useUIStore(state => state.activeModal);
