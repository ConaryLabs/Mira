// frontend/src/hooks/__tests__/useArtifacts.test.ts
// Artifacts Hook Tests

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import { useArtifacts } from '../useArtifacts';
import { useAppState } from '../../stores/useAppState';
import { useWebSocketStore } from '../../stores/useWebSocketStore';

// Mock the stores
vi.mock('../../stores/useAppState', () => ({
  useAppState: vi.fn(),
  useArtifactState: vi.fn(),
}));

vi.mock('../../stores/useWebSocketStore', () => ({
  useWebSocketStore: vi.fn(),
}));

// Import the actual mock
import { useArtifactState } from '../../stores/useAppState';

describe('useArtifacts', () => {
  let mockArtifacts: any[];
  let mockActiveArtifact: any;
  let mockAddArtifact: ReturnType<typeof vi.fn>;
  let mockSetActiveArtifact: ReturnType<typeof vi.fn>;
  let mockUpdateArtifact: ReturnType<typeof vi.fn>;
  let mockRemoveArtifact: ReturnType<typeof vi.fn>;
  let mockSetShowArtifacts: ReturnType<typeof vi.fn>;
  let mockSend: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.clearAllMocks();

    mockArtifacts = [
      {
        id: 'artifact-1',
        path: 'src/test.ts',
        content: 'console.log("test");',
        language: 'typescript',
        status: 'draft',
        timestamp: Date.now(),
      },
      {
        id: 'artifact-2',
        path: 'src/main.ts',
        content: 'export default {};',
        language: 'typescript',
        status: 'saved',
        timestamp: Date.now(),
      },
    ];

    mockActiveArtifact = mockArtifacts[0];
    mockAddArtifact = vi.fn();
    mockSetActiveArtifact = vi.fn();
    mockUpdateArtifact = vi.fn();
    mockRemoveArtifact = vi.fn();
    mockSetShowArtifacts = vi.fn();
    mockSend = vi.fn().mockResolvedValue(undefined);

    // Setup useArtifactState mock
    (useArtifactState as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
      artifacts: mockArtifacts,
      activeArtifact: mockActiveArtifact,
      showArtifacts: true,
      addArtifact: mockAddArtifact,
      setActiveArtifact: mockSetActiveArtifact,
      updateArtifact: mockUpdateArtifact,
      removeArtifact: mockRemoveArtifact,
    });

    // Setup useAppState mock
    (useAppState as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
      setShowArtifacts: mockSetShowArtifacts,
      currentProject: { id: 'project-123', name: 'Test Project' },
    });

    // Setup useWebSocketStore mock
    (useWebSocketStore as unknown as ReturnType<typeof vi.fn>).mockImplementation((selector) => {
      if (typeof selector === 'function') {
        return selector({ send: mockSend });
      }
      return { send: mockSend };
    });
  });

  describe('basic accessors', () => {
    it('should return artifacts from store', () => {
      const { result } = renderHook(() => useArtifacts());

      expect(result.current.artifacts).toEqual(mockArtifacts);
    });

    it('should return active artifact from store', () => {
      const { result } = renderHook(() => useArtifacts());

      expect(result.current.activeArtifact).toEqual(mockActiveArtifact);
    });

    it('should return showArtifacts flag from store', () => {
      const { result } = renderHook(() => useArtifacts());

      expect(result.current.showArtifacts).toBe(true);
    });
  });

  describe('closeArtifacts', () => {
    it('should set showArtifacts to false', () => {
      const { result } = renderHook(() => useArtifacts());

      act(() => {
        result.current.closeArtifacts();
      });

      expect(mockSetShowArtifacts).toHaveBeenCalledWith(false);
    });
  });

  describe('updatePath', () => {
    it('should normalize and update artifact path', () => {
      const { result } = renderHook(() => useArtifacts());

      act(() => {
        result.current.updatePath('artifact-1', './src//new-path.ts');
      });

      expect(mockUpdateArtifact).toHaveBeenCalledWith('artifact-1', {
        path: 'src/new-path.ts',
      });
    });

    it('should handle backslashes in path', () => {
      const { result } = renderHook(() => useArtifacts());

      act(() => {
        result.current.updatePath('artifact-1', 'src\\windows\\path.ts');
      });

      expect(mockUpdateArtifact).toHaveBeenCalledWith('artifact-1', {
        path: 'src/windows/path.ts',
      });
    });
  });

  describe('save', () => {
    it('should send file write command to backend', async () => {
      const { result } = renderHook(() => useArtifacts());

      await act(async () => {
        await result.current.save('artifact-1');
      });

      expect(mockSend).toHaveBeenCalledWith({
        type: 'file_system_command',
        method: 'files.write',
        params: {
          project_id: 'project-123',
          path: 'src/test.ts',
          content: 'console.log("test");',
        },
      });
    });

    it('should update artifact status to saved after successful save', async () => {
      const { result } = renderHook(() => useArtifacts());

      await act(async () => {
        await result.current.save('artifact-1');
      });

      expect(mockUpdateArtifact).toHaveBeenCalledWith('artifact-1', {
        path: 'src/test.ts',
        status: 'saved',
        timestamp: expect.any(Number),
      });
    });

    it('should not save if artifact not found', async () => {
      const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
      const { result } = renderHook(() => useArtifacts());

      await act(async () => {
        await result.current.save('non-existent');
      });

      expect(mockSend).not.toHaveBeenCalled();
      expect(consoleSpy).toHaveBeenCalledWith('Artifact not found:', 'non-existent');
      consoleSpy.mockRestore();
    });

    it('should not save if no current project', async () => {
      (useAppState as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        setShowArtifacts: mockSetShowArtifacts,
        currentProject: null,
      });

      const consoleSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
      const { result } = renderHook(() => useArtifacts());

      await act(async () => {
        await result.current.save('artifact-1');
      });

      expect(mockSend).not.toHaveBeenCalled();
      expect(consoleSpy).toHaveBeenCalledWith('Cannot save artifact: no current project');
      consoleSpy.mockRestore();
    });
  });

  describe('apply', () => {
    it('should send file write command to backend', async () => {
      const { result } = renderHook(() => useArtifacts());

      await act(async () => {
        await result.current.apply('artifact-1');
      });

      expect(mockSend).toHaveBeenCalledWith({
        type: 'file_system_command',
        method: 'files.write',
        params: {
          project_id: 'project-123',
          path: 'src/test.ts',
          content: 'console.log("test");',
        },
      });
    });

    it('should update artifact status to applied after successful apply', async () => {
      const { result } = renderHook(() => useArtifacts());

      await act(async () => {
        await result.current.apply('artifact-1');
      });

      expect(mockUpdateArtifact).toHaveBeenCalledWith('artifact-1', {
        path: 'src/test.ts',
        status: 'applied',
        timestamp: expect.any(Number),
      });
    });
  });

  describe('discard', () => {
    it('should remove draft artifacts', () => {
      const { result } = renderHook(() => useArtifacts());

      act(() => {
        result.current.discard('artifact-1'); // draft status
      });

      expect(mockRemoveArtifact).toHaveBeenCalledWith('artifact-1');
    });

    it('should remove saved artifacts', () => {
      const { result } = renderHook(() => useArtifacts());

      act(() => {
        result.current.discard('artifact-2'); // saved status
      });

      expect(mockRemoveArtifact).toHaveBeenCalledWith('artifact-2');
    });

    it('should not crash if artifact not found', () => {
      const { result } = renderHook(() => useArtifacts());

      act(() => {
        result.current.discard('non-existent');
      });

      expect(mockRemoveArtifact).not.toHaveBeenCalled();
    });
  });

  describe('copyArtifact', () => {
    it('should copy artifact content to clipboard', async () => {
      const mockWriteText = vi.fn().mockResolvedValue(undefined);
      Object.assign(navigator, {
        clipboard: { writeText: mockWriteText },
      });

      const { result } = renderHook(() => useArtifacts());

      await act(async () => {
        result.current.copyArtifact('artifact-1');
      });

      expect(mockWriteText).toHaveBeenCalledWith('console.log("test");');
    });

    it('should not crash if artifact not found', () => {
      const mockWriteText = vi.fn();
      Object.assign(navigator, {
        clipboard: { writeText: mockWriteText },
      });

      const { result } = renderHook(() => useArtifacts());

      act(() => {
        result.current.copyArtifact('non-existent');
      });

      expect(mockWriteText).not.toHaveBeenCalled();
    });
  });
});
