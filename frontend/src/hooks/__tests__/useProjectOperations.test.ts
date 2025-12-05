// src/hooks/__tests__/useProjectOperations.test.ts
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';
import { useProjectOperations } from '../useProjectOperations';
import { useAppState } from '../../stores/useAppState';
import { useWebSocketStore } from '../../stores/useWebSocketStore';

// Mock the stores
vi.mock('../../stores/useAppState', () => ({
  useAppState: vi.fn(),
}));

vi.mock('../../stores/useWebSocketStore', () => ({
  useWebSocketStore: vi.fn(),
}));

describe('useProjectOperations', () => {
  let mockSend: ReturnType<typeof vi.fn>;
  let mockAddToast: ReturnType<typeof vi.fn>;
  let mockSetCurrentProject: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    mockSend = vi.fn().mockResolvedValue(undefined);
    mockAddToast = vi.fn();
    mockSetCurrentProject = vi.fn();

    vi.mocked(useAppState).mockReturnValue({
      currentProject: null,
      setCurrentProject: mockSetCurrentProject,
      addToast: mockAddToast,
    } as any);

    vi.mocked(useWebSocketStore).mockReturnValue({
      send: mockSend,
    } as any);
  });

  describe('openDirectory', () => {
    it('opens a directory successfully', async () => {
      const { result } = renderHook(() => useProjectOperations());

      const openPromise = act(async () => {
        return await result.current.openDirectory('/home/user/project');
      });

      await waitFor(() => {
        expect(result.current.opening).toBe(false);
      });

      expect(await openPromise).toBe(true);
      expect(mockSend).toHaveBeenCalledWith({
        type: 'project_command',
        method: 'project.open_directory',
        params: {
          path: '/home/user/project',
        },
      });
      expect(mockAddToast).toHaveBeenCalledWith({
        message: 'Opened project: project',
        type: 'success',
      });
    });

    it('trims directory path', async () => {
      const { result } = renderHook(() => useProjectOperations());

      await act(async () => {
        await result.current.openDirectory('  /home/user/project  ');
      });

      expect(mockSend).toHaveBeenCalledWith({
        type: 'project_command',
        method: 'project.open_directory',
        params: {
          path: '/home/user/project',
        },
      });
    });

    it('rejects empty path', async () => {
      const { result } = renderHook(() => useProjectOperations());

      const success = await act(async () => {
        return await result.current.openDirectory('   ');
      });

      expect(success).toBe(false);
      expect(mockSend).not.toHaveBeenCalled();
      expect(mockAddToast).toHaveBeenCalledWith({
        message: 'Directory path is required',
        type: 'error',
      });
    });

    it('handles open failure', async () => {
      mockSend.mockRejectedValueOnce(new Error('Network error'));
      const { result } = renderHook(() => useProjectOperations());

      const success = await act(async () => {
        return await result.current.openDirectory('/home/user/project');
      });

      expect(success).toBe(false);
      expect(mockAddToast).toHaveBeenCalledWith({
        message: 'Failed to open directory',
        type: 'error',
      });
    });

    it('calls refreshProjects after successful open', async () => {
      vi.useFakeTimers();
      const { result } = renderHook(() => useProjectOperations());

      await act(async () => {
        await result.current.openDirectory('/home/user/project');
      });

      act(() => {
        vi.advanceTimersByTime(100);
      });

      expect(mockSend).toHaveBeenCalledTimes(2);
      expect(mockSend).toHaveBeenNthCalledWith(2, {
        type: 'project_command',
        method: 'project.list',
        params: {},
      });

      vi.useRealTimers();
    });
  });

  describe('createProject', () => {
    it('creates a project successfully', async () => {
      const { result } = renderHook(() => useProjectOperations());

      const createPromise = act(async () => {
        return await result.current.createProject('my-project', 'A test project');
      });

      await waitFor(() => {
        expect(result.current.opening).toBe(false);
      });

      expect(await createPromise).toBe(true);
      expect(mockSend).toHaveBeenCalledWith({
        type: 'project_command',
        method: 'project.create',
        params: {
          name: 'my-project',
          description: 'A test project',
        },
      });
      expect(mockAddToast).toHaveBeenCalledWith({
        message: 'Created project: my-project',
        type: 'success',
      });
    });

    it('creates a project without description', async () => {
      const { result } = renderHook(() => useProjectOperations());

      await act(async () => {
        await result.current.createProject('my-project');
      });

      expect(mockSend).toHaveBeenCalledWith({
        type: 'project_command',
        method: 'project.create',
        params: {
          name: 'my-project',
          description: undefined,
        },
      });
    });

    it('trims project name and description', async () => {
      const { result } = renderHook(() => useProjectOperations());

      await act(async () => {
        await result.current.createProject('  my-project  ', '  description  ');
      });

      expect(mockSend).toHaveBeenCalledWith({
        type: 'project_command',
        method: 'project.create',
        params: {
          name: 'my-project',
          description: 'description',
        },
      });
    });

    it('rejects empty project name', async () => {
      const { result } = renderHook(() => useProjectOperations());

      const success = await act(async () => {
        return await result.current.createProject('   ');
      });

      expect(success).toBe(false);
      expect(mockSend).not.toHaveBeenCalled();
      expect(mockAddToast).toHaveBeenCalledWith({
        message: 'Project name is required',
        type: 'error',
      });
    });

    it('handles create failure', async () => {
      mockSend.mockRejectedValueOnce(new Error('Network error'));
      const { result } = renderHook(() => useProjectOperations());

      const success = await act(async () => {
        return await result.current.createProject('my-project');
      });

      expect(success).toBe(false);
      expect(mockAddToast).toHaveBeenCalledWith({
        message: 'Failed to create project',
        type: 'error',
      });
    });

    it('calls refreshProjects after successful creation', async () => {
      vi.useFakeTimers();
      const { result } = renderHook(() => useProjectOperations());

      await act(async () => {
        await result.current.createProject('my-project');
      });

      // Fast-forward past the setTimeout delay
      act(() => {
        vi.advanceTimersByTime(100);
      });

      // Should have called send twice: once for create, once for refresh
      expect(mockSend).toHaveBeenCalledTimes(2);
      expect(mockSend).toHaveBeenNthCalledWith(2, {
        type: 'project_command',
        method: 'project.list',
        params: {},
      });

      vi.useRealTimers();
    });
  });

  describe('deleteProject', () => {
    it('deletes a project successfully', async () => {
      const { result } = renderHook(() => useProjectOperations());

      const deletePromise = act(async () => {
        return await result.current.deleteProject('project-123');
      });

      await waitFor(() => {
        expect(result.current.deleting).toBe(null);
      });

      expect(await deletePromise).toBe(true);
      expect(mockSend).toHaveBeenCalledWith({
        type: 'project_command',
        method: 'project.delete',
        params: { id: 'project-123' },
      });
      expect(mockAddToast).toHaveBeenCalledWith({
        message: 'Project deleted',
        type: 'success',
      });
    });

    it('clears current project if deleted project is current', async () => {
      vi.mocked(useAppState).mockReturnValue({
        currentProject: { id: 'project-123', name: 'Test' } as any,
        setCurrentProject: mockSetCurrentProject,
        addToast: mockAddToast,
      } as any);

      const { result } = renderHook(() => useProjectOperations());

      await act(async () => {
        await result.current.deleteProject('project-123');
      });

      expect(mockSetCurrentProject).toHaveBeenCalledWith(null);
    });

    it('does not clear current project if different project deleted', async () => {
      vi.mocked(useAppState).mockReturnValue({
        currentProject: { id: 'project-456', name: 'Other' } as any,
        setCurrentProject: mockSetCurrentProject,
        addToast: mockAddToast,
      } as any);

      const { result } = renderHook(() => useProjectOperations());

      await act(async () => {
        await result.current.deleteProject('project-123');
      });

      expect(mockSetCurrentProject).not.toHaveBeenCalled();
    });

    it('handles delete failure', async () => {
      mockSend.mockRejectedValueOnce(new Error('Network error'));
      const { result } = renderHook(() => useProjectOperations());

      const success = await act(async () => {
        return await result.current.deleteProject('project-123');
      });

      expect(success).toBe(false);
      expect(mockAddToast).toHaveBeenCalledWith({
        message: 'Failed to delete project',
        type: 'error',
      });
    });

    it('calls refreshProjects after successful deletion', async () => {
      vi.useFakeTimers();
      const { result } = renderHook(() => useProjectOperations());

      await act(async () => {
        await result.current.deleteProject('project-123');
      });

      act(() => {
        vi.advanceTimersByTime(100);
      });

      expect(mockSend).toHaveBeenCalledTimes(2);
      expect(mockSend).toHaveBeenNthCalledWith(2, {
        type: 'project_command',
        method: 'project.list',
        params: {},
      });

      vi.useRealTimers();
    });
  });

  describe('selectProject', () => {
    it('selects a project', () => {
      const { result } = renderHook(() => useProjectOperations());
      const project = { id: 'project-123', name: 'Test Project' } as any;

      act(() => {
        result.current.selectProject(project);
      });

      expect(mockSetCurrentProject).toHaveBeenCalledWith(project);
    });
  });

  describe('refreshProjects', () => {
    it('sends project.list command', () => {
      const { result } = renderHook(() => useProjectOperations());

      act(() => {
        result.current.refreshProjects();
      });

      expect(mockSend).toHaveBeenCalledWith({
        type: 'project_command',
        method: 'project.list',
        params: {},
      });
    });
  });

  describe('loading states', () => {
    it('initializes with correct default states', () => {
      const { result } = renderHook(() => useProjectOperations());

      expect(result.current.opening).toBe(false);
      expect(result.current.deleting).toBe(null);
    });

    it('manages opening state correctly', async () => {
      const { result } = renderHook(() => useProjectOperations());

      expect(result.current.opening).toBe(false);

      const openPromise = act(async () => {
        return await result.current.openDirectory('/home/user/project');
      });

      await waitFor(() => {
        expect(result.current.opening).toBe(false);
      });

      await openPromise;
    });

    it('manages deleting state correctly', async () => {
      const { result } = renderHook(() => useProjectOperations());

      expect(result.current.deleting).toBe(null);

      const deletePromise = act(async () => {
        return await result.current.deleteProject('project-123');
      });

      await waitFor(() => {
        expect(result.current.deleting).toBe(null);
      });

      await deletePromise;
    });
  });
});
