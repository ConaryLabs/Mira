// Theme store for Mira Studio
// Uses Svelte writable stores with localStorage persistence

import { writable, derived, get } from 'svelte/store';
import { browser } from '$app/environment';

export type ThemeName = 'terminal-dark' | 'terminal-retro' | 'terminal-modern' | 'terminal-neon';

export interface ThemeColors {
  bg: string;
  bgSecondary: string;
  text: string;
  textDim: string;
  accent: string;
  prompt: string;
  success: string;
  error: string;
  warning: string;
  border: string;
}

export const themes: Record<ThemeName, ThemeColors> = {
  'terminal-dark': {
    bg: '#0d1117',
    bgSecondary: '#161b22',
    text: '#c9d1d9',
    textDim: '#8b949e',
    accent: '#58a6ff',
    prompt: '#7ee787',
    success: '#3fb950',
    error: '#f85149',
    warning: '#d29922',
    border: '#30363d',
  },
  'terminal-retro': {
    bg: '#0a0a0a',
    bgSecondary: '#111111',
    text: '#33ff33',
    textDim: '#1a991a',
    accent: '#33ff33',
    prompt: '#33ff33',
    success: '#33ff33',
    error: '#ff3333',
    warning: '#ffff33',
    border: '#1a331a',
  },
  'terminal-modern': {
    bg: '#1a1b26',
    bgSecondary: '#24283b',
    text: '#a9b1d6',
    textDim: '#565f89',
    accent: '#7aa2f7',
    prompt: '#9ece6a',
    success: '#9ece6a',
    error: '#f7768e',
    warning: '#e0af68',
    border: '#3b4261',
  },
  'terminal-neon': {
    bg: '#0a0014',
    bgSecondary: '#150025',
    text: '#f0f0f0',
    textDim: '#888888',
    accent: '#ff00ff',
    prompt: '#00ffff',
    success: '#00ff00',
    error: '#ff0055',
    warning: '#ffff00',
    border: '#ff00ff44',
  },
};

const STORAGE_KEY = 'mira-theme';

function getInitialTheme(): ThemeName {
  if (!browser) return 'terminal-dark';
  const stored = localStorage.getItem(STORAGE_KEY);
  if (stored && stored in themes) return stored as ThemeName;
  return 'terminal-dark';
}

function applyTheme(theme: ThemeName) {
  if (!browser) return;
  const root = document.documentElement;
  root.setAttribute('data-theme', theme);

  const colors = themes[theme];
  root.style.setProperty('--term-bg', colors.bg);
  root.style.setProperty('--term-bg-secondary', colors.bgSecondary);
  root.style.setProperty('--term-text', colors.text);
  root.style.setProperty('--term-text-dim', colors.textDim);
  root.style.setProperty('--term-accent', colors.accent);
  root.style.setProperty('--term-prompt', colors.prompt);
  root.style.setProperty('--term-success', colors.success);
  root.style.setProperty('--term-error', colors.error);
  root.style.setProperty('--term-warning', colors.warning);
  root.style.setProperty('--term-border', colors.border);
}

// Create the store
function createThemeStore() {
  const { subscribe, set, update } = writable<ThemeName>(getInitialTheme());

  return {
    subscribe,
    set: (theme: ThemeName) => {
      set(theme);
      if (browser) {
        localStorage.setItem(STORAGE_KEY, theme);
        applyTheme(theme);
      }
    },
    init: () => {
      const theme = getInitialTheme();
      set(theme);
      applyTheme(theme);
    },
  };
}

export const currentTheme = createThemeStore();

export const themeColors = derived(currentTheme, ($theme) => themes[$theme]);

export const themeNames = Object.keys(themes) as ThemeName[];

// Helper for components
export function useTheme() {
  return {
    current: currentTheme,
    colors: themeColors,
    set: currentTheme.set,
    themes: themeNames,
  };
}
