// src/stores/useAuthStore.ts
// Authentication state with real JWT-based auth

import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import { APP_CONFIG } from '../config/app';
import { useThemeStore } from './useThemeStore';

interface User {
  id: string;
  username: string;
  displayName: string;
  email?: string;
  theme_preference?: string;
}

interface AuthResponse {
  user: User;
  token: string;
}

interface AuthState {
  user: User | null;
  token: string | null;
  isAuthenticated: boolean;

  login: (username: string, password: string) => Promise<boolean>;
  register: (username: string, password: string, email?: string, displayName?: string) => Promise<boolean>;
  logout: () => void;
  setUser: (user: User, token: string) => void;
  verifyToken: () => Promise<boolean>;
  changePassword: (currentPassword: string, newPassword: string) => Promise<{ success: boolean; error?: string }>;
}

export const useAuthStore = create<AuthState>()(
  persist(
    (set, get) => ({
      user: null,
      token: null,
      isAuthenticated: false,

      login: async (username: string, password: string) => {
        try {
          const response = await fetch('/api/auth/login', {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
            },
            body: JSON.stringify({ username, password }),
          });

          if (!response.ok) {
            const error = await response.json();
            console.error('Login failed:', error);
            return false;
          }

          const data: AuthResponse = await response.json();

          set({
            user: data.user,
            token: data.token,
            isAuthenticated: true,
          });

          // Initialize theme from user preference
          useThemeStore.getState().initializeFromUser(data.user.theme_preference);

          return true;
        } catch (error) {
          console.error('Login error:', error);
          return false;
        }
      },

      register: async (username: string, password: string, email?: string, displayName?: string) => {
        try {
          const response = await fetch('/api/auth/register', {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
            },
            body: JSON.stringify({ username, password, email, display_name: displayName }),
          });

          if (!response.ok) {
            const error = await response.json();
            console.error('Registration failed:', error);
            return false;
          }

          const data: AuthResponse = await response.json();

          set({
            user: data.user,
            token: data.token,
            isAuthenticated: true,
          });

          // Initialize theme from user preference (new users get light mode)
          useThemeStore.getState().initializeFromUser(data.user.theme_preference);

          return true;
        } catch (error) {
          console.error('Registration error:', error);
          return false;
        }
      },

      verifyToken: async () => {
        const { token } = get();
        if (!token) return false;

        try {
          const response = await fetch('/api/auth/verify', {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
            },
            body: JSON.stringify({ token }),
          });

          if (!response.ok) {
            set({ user: null, token: null, isAuthenticated: false });
            return false;
          }

          const data = await response.json();
          return data.valid === true;
        } catch (error) {
          console.error('Token verification error:', error);
          set({ user: null, token: null, isAuthenticated: false });
          return false;
        }
      },

      logout: () => {
        set({
          user: null,
          token: null,
          isAuthenticated: false,
        });
      },

      changePassword: async (currentPassword: string, newPassword: string) => {
        const { token } = get();
        if (!token) {
          return { success: false, error: 'Not authenticated' };
        }

        try {
          const response = await fetch('/api/auth/change-password', {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
              'Authorization': `Bearer ${token}`,
            },
            body: JSON.stringify({
              current_password: currentPassword,
              new_password: newPassword,
            }),
          });

          if (!response.ok) {
            const error = await response.json();
            return { success: false, error: error.error || 'Failed to change password' };
          }

          return { success: true };
        } catch (error) {
          console.error('Change password error:', error);
          return { success: false, error: 'An error occurred' };
        }
      },

      setUser: (user: User, token: string) => {
        set({
          user,
          token,
          isAuthenticated: true,
        });
      },
    }),
    {
      name: 'mira-auth-storage', // LocalStorage key
    }
  )
);

// Selector hooks for components
export const useCurrentUser = () => useAuthStore(state => state.user);
export const useIsAuthenticated = () => useAuthStore(state => state.isAuthenticated);
export const useToken = () => useAuthStore(state => state.token);

// Expose auth store globally for WebSocket access
if (typeof window !== 'undefined') {
  (window as any).__authStore = useAuthStore;
}
