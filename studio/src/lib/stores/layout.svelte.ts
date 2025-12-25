/**
 * Layout State Store
 *
 * Manages UI panel state with localStorage persistence:
 * - Context drawer (right panel) state
 * - Settings modal state
 * - Mobile responsive breakpoints
 */

import { browser } from '$app/environment';

export type DrawerTab = 'timeline' | 'workspace';

export interface LayoutState {
  contextDrawer: {
    open: boolean;
    width: number;
    activeTab: DrawerTab;
  };
  settingsOpen: boolean;
  isMobile: boolean;
}

// Default state
const defaultState: LayoutState = {
  contextDrawer: {
    open: true,  // Open by default on desktop
    width: 360,
    activeTab: 'timeline',
  },
  settingsOpen: false,
  isMobile: false,
};

// Load from localStorage if available
function loadState(): LayoutState {
  if (!browser) return { ...defaultState };

  try {
    const saved = localStorage.getItem('mira-layout');
    if (saved) {
      const parsed = JSON.parse(saved);
      return {
        ...defaultState,
        ...parsed,
        isMobile: window.innerWidth < 768,
      };
    }
  } catch {
    // Ignore parse errors
  }

  return {
    ...defaultState,
    isMobile: browser ? window.innerWidth < 768 : false,
  };
}

// Reactive state
let state = $state<LayoutState>(loadState());

// Persist to localStorage (debounced)
let saveTimeout: number | null = null;
function persistState() {
  if (!browser) return;

  if (saveTimeout) {
    clearTimeout(saveTimeout);
  }
  saveTimeout = window.setTimeout(() => {
    const toSave = {
      contextDrawer: state.contextDrawer,
      // Don't persist settingsOpen or isMobile
    };
    localStorage.setItem('mira-layout', JSON.stringify(toSave));
  }, 100);
}

/**
 * Layout store with derived values and actions
 */
export const layoutStore = {
  // State access
  get state() { return state; },
  get contextDrawer() { return state.contextDrawer; },
  get settingsOpen() { return state.settingsOpen; },
  get isMobile() { return state.isMobile; },

  // Derived: is drawer effectively visible (always available, even on mobile)
  get isDrawerVisible() {
    return state.contextDrawer.open;
  },

  // Derived: is drawer in bottom sheet mode
  get isBottomSheet() {
    return state.isMobile;
  },

  // Actions
  toggleDrawer() {
    state.contextDrawer.open = !state.contextDrawer.open;
    persistState();
  },

  openDrawer(tab?: DrawerTab) {
    state.contextDrawer.open = true;
    if (tab) {
      state.contextDrawer.activeTab = tab;
    }
    persistState();
  },

  closeDrawer() {
    state.contextDrawer.open = false;
    persistState();
  },

  setDrawerTab(tab: DrawerTab) {
    state.contextDrawer.activeTab = tab;
    // Auto-open if setting tab
    if (!state.contextDrawer.open) {
      state.contextDrawer.open = true;
    }
    persistState();
  },

  setDrawerWidth(width: number) {
    state.contextDrawer.width = Math.max(280, Math.min(600, width));
    persistState();
  },

  toggleSettings() {
    state.settingsOpen = !state.settingsOpen;
  },

  openSettings() {
    state.settingsOpen = true;
  },

  closeSettings() {
    state.settingsOpen = false;
  },

  setMobile(isMobile: boolean) {
    state.isMobile = isMobile;
    // Auto-close drawer on mobile
    if (isMobile && state.contextDrawer.open) {
      state.contextDrawer.open = false;
    }
  },

  // Initialize on mount (call from root layout)
  init() {
    if (!browser) return;

    const checkMobile = () => {
      this.setMobile(window.innerWidth < 768);
    };

    checkMobile();
    window.addEventListener('resize', checkMobile);

    return () => {
      window.removeEventListener('resize', checkMobile);
    };
  },
};
