// frontend/src/stores/__tests__/authStore.test.ts
// Authentication Store Tests

import { describe, it, expect, beforeEach, vi } from 'vitest';
import { useAuthStore } from '../useAuthStore';

// Mock fetch globally
const mockFetch = vi.fn();
global.fetch = mockFetch;

describe('useAuthStore', () => {
  beforeEach(() => {
    // Reset store state before each test
    useAuthStore.setState({
      user: null,
      token: null,
      isAuthenticated: false,
    });
    mockFetch.mockReset();
  });

  describe('initial state', () => {
    it('should have null user and token initially', () => {
      const state = useAuthStore.getState();

      expect(state.user).toBeNull();
      expect(state.token).toBeNull();
      expect(state.isAuthenticated).toBe(false);
    });
  });

  describe('login', () => {
    it('should set user and token on successful login', async () => {
      const mockUser = {
        id: 'user-123',
        username: 'testuser',
        displayName: 'Test User',
        email: 'test@example.com',
      };
      const mockToken = 'jwt-token-abc123';

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ user: mockUser, token: mockToken }),
      });

      const result = await useAuthStore.getState().login('testuser', 'password123');

      expect(result).toBe(true);
      expect(useAuthStore.getState().user).toEqual(mockUser);
      expect(useAuthStore.getState().token).toBe(mockToken);
      expect(useAuthStore.getState().isAuthenticated).toBe(true);
    });

    it('should return false on failed login', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: false,
        json: async () => ({ error: 'Invalid credentials' }),
      });

      const result = await useAuthStore.getState().login('baduser', 'wrongpass');

      expect(result).toBe(false);
      expect(useAuthStore.getState().user).toBeNull();
      expect(useAuthStore.getState().token).toBeNull();
      expect(useAuthStore.getState().isAuthenticated).toBe(false);
    });

    it('should return false on network error', async () => {
      mockFetch.mockRejectedValueOnce(new Error('Network error'));

      const result = await useAuthStore.getState().login('testuser', 'password');

      expect(result).toBe(false);
      expect(useAuthStore.getState().isAuthenticated).toBe(false);
    });
  });

  describe('register', () => {
    it('should set user and token on successful registration', async () => {
      const mockUser = {
        id: 'user-456',
        username: 'newuser',
        displayName: 'New User',
        email: 'new@example.com',
      };
      const mockToken = 'jwt-token-def456';

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ user: mockUser, token: mockToken }),
      });

      const result = await useAuthStore.getState().register(
        'newuser',
        'password123',
        'new@example.com',
        'New User'
      );

      expect(result).toBe(true);
      expect(useAuthStore.getState().user).toEqual(mockUser);
      expect(useAuthStore.getState().token).toBe(mockToken);
      expect(useAuthStore.getState().isAuthenticated).toBe(true);
    });

    it('should return false on registration failure', async () => {
      mockFetch.mockResolvedValueOnce({
        ok: false,
        json: async () => ({ error: 'Username already exists' }),
      });

      const result = await useAuthStore.getState().register(
        'existinguser',
        'password123'
      );

      expect(result).toBe(false);
      expect(useAuthStore.getState().isAuthenticated).toBe(false);
    });
  });

  describe('logout', () => {
    it('should clear user, token, and isAuthenticated', () => {
      // Set up authenticated state
      useAuthStore.setState({
        user: { id: 'user-123', username: 'test', displayName: 'Test' },
        token: 'some-token',
        isAuthenticated: true,
      });

      useAuthStore.getState().logout();

      expect(useAuthStore.getState().user).toBeNull();
      expect(useAuthStore.getState().token).toBeNull();
      expect(useAuthStore.getState().isAuthenticated).toBe(false);
    });
  });

  describe('setUser', () => {
    it('should set user and token directly', () => {
      const user = { id: 'user-789', username: 'directuser', displayName: 'Direct User' };
      const token = 'direct-token';

      useAuthStore.getState().setUser(user, token);

      expect(useAuthStore.getState().user).toEqual(user);
      expect(useAuthStore.getState().token).toBe(token);
      expect(useAuthStore.getState().isAuthenticated).toBe(true);
    });
  });

  describe('verifyToken', () => {
    it('should return true for valid token', async () => {
      useAuthStore.setState({
        user: { id: 'user-123', username: 'test', displayName: 'Test' },
        token: 'valid-token',
        isAuthenticated: true,
      });

      mockFetch.mockResolvedValueOnce({
        ok: true,
        json: async () => ({ valid: true }),
      });

      const result = await useAuthStore.getState().verifyToken();

      expect(result).toBe(true);
      expect(useAuthStore.getState().isAuthenticated).toBe(true);
    });

    it('should return false and clear state for invalid token', async () => {
      useAuthStore.setState({
        user: { id: 'user-123', username: 'test', displayName: 'Test' },
        token: 'expired-token',
        isAuthenticated: true,
      });

      mockFetch.mockResolvedValueOnce({
        ok: false,
        json: async () => ({ valid: false }),
      });

      const result = await useAuthStore.getState().verifyToken();

      expect(result).toBe(false);
      expect(useAuthStore.getState().user).toBeNull();
      expect(useAuthStore.getState().token).toBeNull();
      expect(useAuthStore.getState().isAuthenticated).toBe(false);
    });

    it('should return false when no token exists', async () => {
      const result = await useAuthStore.getState().verifyToken();

      expect(result).toBe(false);
    });

    it('should clear state on network error during verification', async () => {
      useAuthStore.setState({
        user: { id: 'user-123', username: 'test', displayName: 'Test' },
        token: 'some-token',
        isAuthenticated: true,
      });

      mockFetch.mockRejectedValueOnce(new Error('Network error'));

      const result = await useAuthStore.getState().verifyToken();

      expect(result).toBe(false);
      expect(useAuthStore.getState().user).toBeNull();
      expect(useAuthStore.getState().isAuthenticated).toBe(false);
    });
  });
});
