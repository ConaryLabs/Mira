// src/stores/useThemeStore.ts
// Theme state management with backend persistence

import { create } from 'zustand';

type Theme = 'light' | 'dark';

interface ThemeState {
  theme: Theme;
  setTheme: (theme: Theme) => void;
  toggleTheme: () => void;
  initializeFromUser: (themePreference: string | null | undefined) => void;
}

// Apply theme class to document
function applyTheme(theme: Theme) {
  if (typeof document !== 'undefined') {
    if (theme === 'dark') {
      document.documentElement.classList.add('dark');
    } else {
      document.documentElement.classList.remove('dark');
    }
  }
}

// Save theme preference to backend
async function saveThemePreference(theme: Theme) {
  // Get token from auth store
  const authStore = (window as any).__authStore;
  const token = authStore?.getState?.().token;

  if (!token) {
    // Not logged in, can't save
    return;
  }

  try {
    await fetch('/api/auth/preferences', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'Authorization': `Bearer ${token}`,
      },
      body: JSON.stringify({ theme_preference: theme }),
    });
  } catch (error) {
    console.error('Failed to save theme preference:', error);
  }
}

export const useThemeStore = create<ThemeState>((set, get) => ({
  // Default to light mode
  theme: 'light',

  setTheme: (theme: Theme) => {
    applyTheme(theme);
    set({ theme });
    saveThemePreference(theme);
  },

  toggleTheme: () => {
    const currentTheme = get().theme;
    const newTheme: Theme = currentTheme === 'light' ? 'dark' : 'light';
    applyTheme(newTheme);
    set({ theme: newTheme });
    saveThemePreference(newTheme);
  },

  // Called when user logs in to initialize theme from their saved preference
  initializeFromUser: (themePreference: string | null | undefined) => {
    const theme: Theme = themePreference === 'dark' ? 'dark' : 'light';
    applyTheme(theme);
    set({ theme });
  },
}));

// Initialize theme on load (default to light)
if (typeof document !== 'undefined') {
  // Remove dark class initially (light mode default)
  document.documentElement.classList.remove('dark');
}

// Selector hooks
export const useTheme = () => useThemeStore(state => state.theme);
export const useToggleTheme = () => useThemeStore(state => state.toggleTheme);
