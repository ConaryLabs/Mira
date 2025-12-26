/**
 * Layout State Store
 *
 * Manages UI panel state with localStorage persistence:
 * - Left nav state (collapsed | expanded | settings) - enforces single container invariant
 * - Context drawer (right panel) state
 * - Mobile responsive breakpoints
 */

import { browser } from '$app/environment';

export type DrawerTab = 'timeline' | 'workspace' | 'advisory' | 'orchestration';

// Left nav can only be in ONE of these states - prevents double-sidebar bug
export type LeftNavState = 'collapsed' | 'expanded' | 'settings';

export interface LayoutState {
  leftNav: LeftNavState;
  contextDrawer: {
    open: boolean;
    width: number;
    activeTab: DrawerTab;
  };
  isMobile: boolean;
}

// Default state
const defaultState: LayoutState = {
  leftNav: 'collapsed',  // Start collapsed, user can expand
  contextDrawer: {
    open: true,  // Open by default on desktop
    width: 360,
    activeTab: 'timeline',
  },
  isMobile: false,
};

// Load from localStorage if available
function loadState(): LayoutState {
  if (!browser) return { ...defaultState };

  try {
    const saved = localStorage.getItem('mira-layout');
    if (saved) {
      const parsed = JSON.parse(saved);
      // Migrate from old settingsOpen boolean if present
      let leftNav: LeftNavState = parsed.leftNav || defaultState.leftNav;
      if (parsed.settingsOpen === true) {
        leftNav = 'settings';
      }
      return {
        ...defaultState,
        leftNav,
        contextDrawer: parsed.contextDrawer || defaultState.contextDrawer,
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
      leftNav: state.leftNav,
      contextDrawer: state.contextDrawer,
      // Don't persist isMobile (computed from window size)
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
  get leftNav() { return state.leftNav; },
  get contextDrawer() { return state.contextDrawer; },
  get isMobile() { return state.isMobile; },

  // Derived: is settings panel showing (in expanded left nav)
  get settingsOpen() { return state.leftNav === 'settings'; },

  // Derived: is left nav expanded (either expanded or settings)
  get isLeftNavExpanded() {
    return state.leftNav === 'expanded' || state.leftNav === 'settings';
  },

  // Derived: is drawer effectively visible
  get isDrawerVisible() {
    return state.contextDrawer.open;
  },

  // Derived: is drawer in bottom sheet mode
  get isBottomSheet() {
    return state.isMobile;
  },

  // Left nav actions
  setLeftNav(navState: LeftNavState) {
    state.leftNav = navState;
    persistState();
  },

  toggleLeftNav() {
    // Toggle between collapsed and expanded (not settings)
    state.leftNav = state.leftNav === 'collapsed' ? 'expanded' : 'collapsed';
    persistState();
  },

  toggleSettings() {
    // Toggle between settings and collapsed
    state.leftNav = state.leftNav === 'settings' ? 'collapsed' : 'settings';
    persistState();
  },

  openSettings() {
    state.leftNav = 'settings';
    persistState();
  },

  closeSettings() {
    state.leftNav = 'collapsed';
    persistState();
  },

  // Context drawer actions
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

  setMobile(isMobile: boolean) {
    state.isMobile = isMobile;
    // Auto-close drawer on mobile
    if (isMobile && state.contextDrawer.open) {
      state.contextDrawer.open = false;
    }
    // Collapse left nav on mobile
    if (isMobile && state.leftNav !== 'collapsed') {
      state.leftNav = 'collapsed';
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
