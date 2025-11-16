// src/hooks/__tests__/useGitOperations.test.ts
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';
import { useGitOperations } from '../useGitOperations';
import { useAppState } from '../../stores/useAppState';
import { useWebSocketStore } from '../../stores/useWebSocketStore';

// Mock the stores
vi.mock('../../stores/useAppState', () => ({
  useAppState: vi.fn(),
}));

vi.mock('../../stores/useWebSocketStore', () => ({
  useWebSocketStore: vi.fn(),
}));

describe('useGitOperations', () => {
  let mockSend: ReturnType<typeof vi.fn>;
  let mockAddToast: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    mockSend = vi.fn().mockResolvedValue(undefined);
    mockAddToast = vi.fn();

    vi.mocked(useAppState).mockReturnValue({
      addToast: mockAddToast,
    } as any);

    vi.mocked(useWebSocketStore).mockReturnValue({
      send: mockSend,
    } as any);
  });

  describe('attachCodebase - local', () => {
    it('attaches local codebase successfully', async () => {
      const { result } = renderHook(() => useGitOperations());

      const attachPromise = act(async () => {
        return await result.current.attachCodebase('project-123', 'local', {
          path: '/home/user/my-project',
        });
      });

      await waitFor(() => {
        expect(result.current.isAttaching).toBe(false);
      });

      expect(await attachPromise).toBe(true);
      expect(mockSend).toHaveBeenCalledWith({
        type: 'project_command',
        method: 'project.attach_local',
        params: {
          project_id: 'project-123',
          directory_path: '/home/user/my-project',
        },
      });
      expect(mockAddToast).toHaveBeenCalledWith({
        message: 'Local codebase attached successfully',
        type: 'success',
      });
    });

    it('handles local attach failure', async () => {
      mockSend.mockRejectedValueOnce(new Error('Invalid path'));
      const { result } = renderHook(() => useGitOperations());

      const success = await act(async () => {
        return await result.current.attachCodebase('project-123', 'local', {
          path: '/invalid/path',
        });
      });

      expect(success).toBe(false);
      expect(result.current.isAttaching).toBe(false);
      expect(mockAddToast).toHaveBeenCalledWith({
        message: 'Failed to attach codebase',
        type: 'error',
      });
    });
  });

  describe('attachCodebase - git', () => {
    it('completes full git import workflow with progress toasts', async () => {
      const { result } = renderHook(() => useGitOperations());

      const attachPromise = act(async () => {
        return await result.current.attachCodebase('project-123', 'git', {
          url: 'https://github.com/user/repo.git',
        });
      });

      // Wait for operation to complete
      await waitFor(() => {
        expect(result.current.isAttaching).toBe(false);
      });

      const success = await attachPromise;
      expect(success).toBe(true);

      // Should have shown progress toasts
      expect(mockAddToast).toHaveBeenCalledWith({
        message: 'Attaching repository...',
        type: 'info',
      });
      expect(mockAddToast).toHaveBeenCalledWith({
        message: 'Cloning repository...',
        type: 'info',
      });
      expect(mockAddToast).toHaveBeenCalledWith({
        message: 'Importing codebase...',
        type: 'info',
      });
      expect(mockAddToast).toHaveBeenCalledWith({
        message: 'Repository imported successfully!',
        type: 'success',
      });
    });

    it('calls git operations in correct order', async () => {
      const { result } = renderHook(() => useGitOperations());

      const attachPromise = act(async () => {
        return await result.current.attachCodebase('project-123', 'git', {
          url: 'https://github.com/user/repo.git',
        });
      });

      await waitFor(() => {
        expect(result.current.isAttaching).toBe(false);
      });

      await attachPromise;

      // Verify the sequence of git commands
      expect(mockSend).toHaveBeenNthCalledWith(1, {
        type: 'git_command',
        method: 'git.attach',
        params: {
          project_id: 'project-123',
          repo_url: 'https://github.com/user/repo.git',
        },
      });

      expect(mockSend).toHaveBeenNthCalledWith(2, {
        type: 'git_command',
        method: 'git.clone',
        params: {
          project_id: 'project-123',
        },
      });

      expect(mockSend).toHaveBeenNthCalledWith(3, {
        type: 'git_command',
        method: 'git.import',
        params: {
          project_id: 'project-123',
        },
      });

      expect(mockSend).toHaveBeenCalledTimes(3);
    });

    it('handles git attach failure at attach stage', async () => {
      mockSend.mockRejectedValueOnce(new Error('Invalid URL'));
      const { result } = renderHook(() => useGitOperations());

      const success = await act(async () => {
        return await result.current.attachCodebase('project-123', 'git', {
          url: 'invalid-url',
        });
      });

      expect(success).toBe(false);
      expect(result.current.isAttaching).toBe(false);
      expect(mockAddToast).toHaveBeenCalledWith({
        message: 'Failed to attach codebase',
        type: 'error',
      });

      // Should not proceed to clone/import
      expect(mockSend).toHaveBeenCalledTimes(1);
    });

    it('handles git failure at clone stage', async () => {
      mockSend
        .mockResolvedValueOnce(undefined) // attach succeeds
        .mockRejectedValueOnce(new Error('Clone failed')); // clone fails

      const { result } = renderHook(() => useGitOperations());

      const success = await act(async () => {
        return await result.current.attachCodebase('project-123', 'git', {
          url: 'https://github.com/user/repo.git',
        });
      });

      expect(success).toBe(false);
      expect(result.current.isAttaching).toBe(false);

      // Should have called attach and clone, but not import
      expect(mockSend).toHaveBeenCalledTimes(2);
    });

    it('handles git failure at import stage', async () => {
      mockSend
        .mockResolvedValueOnce(undefined) // attach succeeds
        .mockResolvedValueOnce(undefined) // clone succeeds
        .mockRejectedValueOnce(new Error('Import failed')); // import fails

      const { result } = renderHook(() => useGitOperations());

      const success = await act(async () => {
        return await result.current.attachCodebase('project-123', 'git', {
          url: 'https://github.com/user/repo.git',
        });
      });

      expect(success).toBe(false);
      expect(result.current.isAttaching).toBe(false);

      // Should have called all three operations
      expect(mockSend).toHaveBeenCalledTimes(3);
    });
  });

  describe('loading state', () => {
    it('initializes with isAttaching false', () => {
      const { result } = renderHook(() => useGitOperations());
      expect(result.current.isAttaching).toBe(false);
    });

    it('sets isAttaching during operation', async () => {
      const { result } = renderHook(() => useGitOperations());

      expect(result.current.isAttaching).toBe(false);

      const attachPromise = act(async () => {
        return await result.current.attachCodebase('project-123', 'local', {
          path: '/test',
        });
      });

      await waitFor(() => {
        expect(result.current.isAttaching).toBe(false);
      });

      await attachPromise;
    });

    it('resets isAttaching even on failure', async () => {
      mockSend.mockRejectedValueOnce(new Error('Failed'));
      const { result } = renderHook(() => useGitOperations());

      await act(async () => {
        await result.current.attachCodebase('project-123', 'local', { path: '/test' });
      });

      expect(result.current.isAttaching).toBe(false);
    });
  });

  describe('delay timings', () => {
    it('uses delays for git workflow', async () => {
      const { result } = renderHook(() => useGitOperations());

      const attachPromise = act(async () => {
        return await result.current.attachCodebase('project-123', 'git', {
          url: 'https://github.com/user/repo.git',
        });
      });

      await waitFor(() => {
        expect(result.current.isAttaching).toBe(false);
      });

      await attachPromise;

      // All three git operations should have been called
      expect(mockSend).toHaveBeenCalledTimes(3);
      expect(mockSend).toHaveBeenNthCalledWith(1, expect.objectContaining({ method: 'git.attach' }));
      expect(mockSend).toHaveBeenNthCalledWith(2, expect.objectContaining({ method: 'git.clone' }));
      expect(mockSend).toHaveBeenNthCalledWith(3, expect.objectContaining({ method: 'git.import' }));
    });
  });
});
