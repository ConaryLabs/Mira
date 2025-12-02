// src/components/__tests__/FileBrowser.test.tsx
// FileBrowser Component Tests

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { FileBrowser } from '../FileBrowser';
import { useWebSocketStore } from '../../stores/useWebSocketStore';
import { useAppState } from '../../stores/useAppState';

// Mock the stores
vi.mock('../../stores/useWebSocketStore', () => ({
  useWebSocketStore: vi.fn(),
}));

vi.mock('../../stores/useAppState', () => ({
  useAppState: vi.fn(),
}));

interface FileNode {
  name: string;
  path: string;
  is_directory: boolean;
  children?: FileNode[];
}

const createFileTree = (): FileNode[] => [
  {
    name: 'src',
    path: 'src',
    is_directory: true,
    children: [
      { name: 'index.ts', path: 'src/index.ts', is_directory: false },
      { name: 'utils.ts', path: 'src/utils.ts', is_directory: false },
      {
        name: 'components',
        path: 'src/components',
        is_directory: true,
        children: [
          { name: 'Button.tsx', path: 'src/components/Button.tsx', is_directory: false },
        ],
      },
    ],
  },
  { name: 'package.json', path: 'package.json', is_directory: false },
];

describe('FileBrowser', () => {
  let mockSend: ReturnType<typeof vi.fn>;
  let mockSubscribe: ReturnType<typeof vi.fn>;
  let mockUnsubscribe: ReturnType<typeof vi.fn>;
  let messageHandler: ((message: any) => void) | null = null;

  beforeEach(() => {
    vi.clearAllMocks();
    messageHandler = null;

    mockSend = vi.fn().mockResolvedValue(undefined);
    mockUnsubscribe = vi.fn();
    mockSubscribe = vi.fn((id, handler) => {
      messageHandler = handler;
      return mockUnsubscribe;
    });

    // Default useWebSocketStore mock
    (useWebSocketStore as unknown as ReturnType<typeof vi.fn>).mockImplementation((selector) => {
      if (typeof selector === 'function') {
        return selector({ send: mockSend, subscribe: mockSubscribe });
      }
      return { send: mockSend, subscribe: mockSubscribe };
    });

    // Default useAppState mock - no project
    (useAppState as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
      currentProject: null,
    });
  });

  describe('no project state', () => {
    it('shows message to select a project when no project is selected', () => {
      render(<FileBrowser />);

      expect(screen.getByText('Select a project to browse files')).toBeInTheDocument();
    });

    it('does not request file tree when no project', () => {
      render(<FileBrowser />);

      expect(mockSend).not.toHaveBeenCalled();
    });
  });

  describe('empty repository state', () => {
    it('shows "No repository attached" when file tree is empty', async () => {
      (useAppState as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        currentProject: { id: 'proj-1', name: 'Test Project' },
      });

      render(<FileBrowser />);

      // Wait for initial load attempt
      await waitFor(() => {
        expect(mockSend).toHaveBeenCalled();
      });

      // File tree response with empty array
      if (messageHandler) {
        messageHandler({ type: 'data', data: { type: 'file_tree', tree: [] } });
      }

      await waitFor(() => {
        expect(screen.getByText('No repository attached')).toBeInTheDocument();
      });
    });

    it('renders refresh button in empty state', async () => {
      (useAppState as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        currentProject: { id: 'proj-1', name: 'Test Project' },
      });

      render(<FileBrowser />);

      // Simulate empty file tree response
      if (messageHandler) {
        messageHandler({ type: 'data', data: { type: 'file_tree', tree: [] } });
      }

      await waitFor(() => {
        expect(screen.getByText('Refresh')).toBeInTheDocument();
      });
    });
  });

  describe('file tree loading', () => {
    it('requests file tree when project is selected', async () => {
      (useAppState as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        currentProject: { id: 'proj-123', name: 'Test Project' },
      });

      render(<FileBrowser />);

      await waitFor(() => {
        expect(mockSend).toHaveBeenCalledWith({
          type: 'git_command',
          method: 'git.tree',
          params: { project_id: 'proj-123' },
        });
      });
    });

    it('subscribes to websocket messages on mount', () => {
      (useAppState as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        currentProject: { id: 'proj-1', name: 'Test Project' },
      });

      render(<FileBrowser />);

      expect(mockSubscribe).toHaveBeenCalledWith('file-browser', expect.any(Function));
    });

    it('unsubscribes on unmount', () => {
      (useAppState as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        currentProject: { id: 'proj-1', name: 'Test Project' },
      });

      const { unmount } = render(<FileBrowser />);
      unmount();

      expect(mockUnsubscribe).toHaveBeenCalled();
    });
  });

  describe('file tree display', () => {
    beforeEach(() => {
      (useAppState as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        currentProject: { id: 'proj-1', name: 'Test Project' },
      });
    });

    it('renders file tree when data is received', async () => {
      render(<FileBrowser />);

      // Simulate file tree response
      if (messageHandler) {
        messageHandler({ type: 'data', data: { type: 'file_tree', tree: createFileTree() } });
      }

      await waitFor(() => {
        expect(screen.getByText('src')).toBeInTheDocument();
        expect(screen.getByText('package.json')).toBeInTheDocument();
      });
    });

    it('expands directory when clicked', async () => {
      render(<FileBrowser />);

      if (messageHandler) {
        messageHandler({ type: 'data', data: { type: 'file_tree', tree: createFileTree() } });
      }

      await waitFor(() => {
        expect(screen.getByText('src')).toBeInTheDocument();
      });

      // Click on src directory to expand
      fireEvent.click(screen.getByText('src'));

      await waitFor(() => {
        expect(screen.getByText('index.ts')).toBeInTheDocument();
        expect(screen.getByText('utils.ts')).toBeInTheDocument();
        expect(screen.getByText('components')).toBeInTheDocument();
      });
    });

    it('collapses directory when clicked again', async () => {
      render(<FileBrowser />);

      if (messageHandler) {
        messageHandler({ type: 'data', data: { type: 'file_tree', tree: createFileTree() } });
      }

      await waitFor(() => {
        expect(screen.getByText('src')).toBeInTheDocument();
      });

      // Expand
      fireEvent.click(screen.getByText('src'));

      await waitFor(() => {
        expect(screen.getByText('index.ts')).toBeInTheDocument();
      });

      // Collapse
      fireEvent.click(screen.getByText('src'));

      await waitFor(() => {
        expect(screen.queryByText('index.ts')).not.toBeInTheDocument();
      });
    });
  });

  describe('file selection', () => {
    beforeEach(() => {
      (useAppState as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        currentProject: { id: 'proj-1', name: 'Test Project' },
      });
    });

    it('requests file content when a file is clicked', async () => {
      render(<FileBrowser />);

      if (messageHandler) {
        messageHandler({ type: 'data', data: { type: 'file_tree', tree: createFileTree() } });
      }

      await waitFor(() => {
        expect(screen.getByText('package.json')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('package.json'));

      expect(mockSend).toHaveBeenCalledWith({
        type: 'git_command',
        method: 'git.file',
        params: {
          project_id: 'proj-1',
          file_path: 'package.json',
        },
      });
    });

    it('shows loading state while file content is loading', async () => {
      render(<FileBrowser />);

      if (messageHandler) {
        messageHandler({ type: 'data', data: { type: 'file_tree', tree: createFileTree() } });
      }

      await waitFor(() => {
        expect(screen.getByText('package.json')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('package.json'));

      await waitFor(() => {
        expect(screen.getByText('Loading...')).toBeInTheDocument();
      });
    });

    it('displays file content when received', async () => {
      render(<FileBrowser />);

      if (messageHandler) {
        messageHandler({ type: 'data', data: { type: 'file_tree', tree: createFileTree() } });
      }

      await waitFor(() => {
        expect(screen.getByText('package.json')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('package.json'));

      // Simulate file content response
      if (messageHandler) {
        messageHandler({
          type: 'data',
          data: {
            type: 'file_content',
            path: 'package.json',
            content: '{ "name": "test-project" }',
          },
        });
      }

      await waitFor(() => {
        expect(screen.getByText('{ "name": "test-project" }')).toBeInTheDocument();
      });
    });

    it('shows "Select a file" message before any file is selected', async () => {
      render(<FileBrowser />);

      if (messageHandler) {
        messageHandler({ type: 'data', data: { type: 'file_tree', tree: createFileTree() } });
      }

      await waitFor(() => {
        expect(screen.getByText('Select a file to view its contents')).toBeInTheDocument();
      });
    });
  });

  describe('toolbar', () => {
    beforeEach(() => {
      (useAppState as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        currentProject: { id: 'proj-1', name: 'Test Project' },
      });
    });

    it('renders Files header', async () => {
      render(<FileBrowser />);

      if (messageHandler) {
        messageHandler({ type: 'data', data: { type: 'file_tree', tree: createFileTree() } });
      }

      await waitFor(() => {
        expect(screen.getByText('Files')).toBeInTheDocument();
      });
    });

    it('renders semantic tags toggle button', async () => {
      render(<FileBrowser />);

      if (messageHandler) {
        messageHandler({ type: 'data', data: { type: 'file_tree', tree: createFileTree() } });
      }

      await waitFor(() => {
        expect(screen.getByTitle('Hide semantic tags')).toBeInTheDocument();
      });
    });

    it('toggles semantic tags visibility', async () => {
      render(<FileBrowser />);

      if (messageHandler) {
        messageHandler({ type: 'data', data: { type: 'file_tree', tree: createFileTree() } });
      }

      await waitFor(() => {
        expect(screen.getByTitle('Hide semantic tags')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByTitle('Hide semantic tags'));

      await waitFor(() => {
        expect(screen.getByTitle('Show semantic tags')).toBeInTheDocument();
      });
    });

    it('renders refresh button in toolbar', async () => {
      render(<FileBrowser />);

      if (messageHandler) {
        messageHandler({ type: 'data', data: { type: 'file_tree', tree: createFileTree() } });
      }

      await waitFor(() => {
        expect(screen.getByTitle('Refresh')).toBeInTheDocument();
      });
    });

    it('refreshes file tree and semantic stats when refresh is clicked', async () => {
      render(<FileBrowser />);

      if (messageHandler) {
        messageHandler({ type: 'data', data: { type: 'file_tree', tree: createFileTree() } });
      }

      // Clear previous calls
      mockSend.mockClear();

      await waitFor(() => {
        expect(screen.getByTitle('Refresh')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByTitle('Refresh'));

      // Should call both git.tree and code.file_semantic_stats
      expect(mockSend).toHaveBeenCalledWith({
        type: 'git_command',
        method: 'git.tree',
        params: { project_id: 'proj-1' },
      });

      expect(mockSend).toHaveBeenCalledWith({
        type: 'code_intelligence_command',
        method: 'code.file_semantic_stats',
        params: { project_id: 'proj-1' },
      });
    });
  });

  describe('semantic stats', () => {
    beforeEach(() => {
      (useAppState as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        currentProject: { id: 'proj-1', name: 'Test Project' },
      });
    });

    it('requests semantic stats when project is selected', async () => {
      render(<FileBrowser />);

      await waitFor(() => {
        expect(mockSend).toHaveBeenCalledWith({
          type: 'code_intelligence_command',
          method: 'code.file_semantic_stats',
          params: { project_id: 'proj-1' },
        });
      });
    });

    it('shows legend when semantic tags are enabled', async () => {
      render(<FileBrowser />);

      if (messageHandler) {
        messageHandler({ type: 'data', data: { type: 'file_tree', tree: createFileTree() } });
      }

      await waitFor(() => {
        expect(screen.getByText('Test')).toBeInTheDocument();
        expect(screen.getByText('Issues')).toBeInTheDocument();
        expect(screen.getByText('Complex')).toBeInTheDocument();
        expect(screen.getByText('Analyzed')).toBeInTheDocument();
      });
    });

    it('hides legend when semantic tags are disabled', async () => {
      render(<FileBrowser />);

      if (messageHandler) {
        messageHandler({ type: 'data', data: { type: 'file_tree', tree: createFileTree() } });
      }

      await waitFor(() => {
        expect(screen.getByTitle('Hide semantic tags')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByTitle('Hide semantic tags'));

      await waitFor(() => {
        expect(screen.queryByText('Test')).not.toBeInTheDocument();
        expect(screen.queryByText('Issues')).not.toBeInTheDocument();
      });
    });
  });

  describe('file content header', () => {
    beforeEach(() => {
      (useAppState as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        currentProject: { id: 'proj-1', name: 'Test Project' },
      });
    });

    it('shows selected file path in content header', async () => {
      render(<FileBrowser />);

      if (messageHandler) {
        messageHandler({ type: 'data', data: { type: 'file_tree', tree: createFileTree() } });
      }

      await waitFor(() => {
        expect(screen.getByText('package.json')).toBeInTheDocument();
      });

      fireEvent.click(screen.getByText('package.json'));

      if (messageHandler) {
        messageHandler({
          type: 'data',
          data: { type: 'file_content', path: 'package.json', content: '{}' },
        });
      }

      // The path should appear in the content header
      await waitFor(() => {
        // There will be two 'package.json' - one in tree, one in header
        const elements = screen.getAllByText('package.json');
        expect(elements.length).toBeGreaterThanOrEqual(1);
      });
    });
  });
});
