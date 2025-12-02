// src/components/__tests__/ActivityPanel.test.tsx
// ActivityPanel Component Tests

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { ActivityPanel } from '../ActivityPanel';
import { useActivityStore } from '../../stores/useActivityStore';
import { useChatStore } from '../../stores/useChatStore';

// Mock the stores
vi.mock('../../stores/useActivityStore', () => ({
  useActivityStore: vi.fn(),
}));

vi.mock('../../stores/useChatStore', () => ({
  useChatStore: vi.fn(),
}));

// Mock the child components
vi.mock('../ActivitySections/ReasoningSection', () => ({
  ReasoningSection: ({ plan }: { plan: any }) => (
    plan ? <div data-testid="reasoning-section">Reasoning: {plan.plan_text}</div> : null
  ),
}));

vi.mock('../ActivitySections/TasksSection', () => ({
  TasksSection: ({ tasks }: { tasks: any[] }) => (
    tasks.length > 0 ? <div data-testid="tasks-section">Tasks: {tasks.length}</div> : null
  ),
}));

vi.mock('../ActivitySections/ToolExecutionsSection', () => ({
  ToolExecutionsSection: ({ toolExecutions }: { toolExecutions: any[] }) => (
    toolExecutions.length > 0 ? <div data-testid="tool-executions-section">Tools: {toolExecutions.length}</div> : null
  ),
}));

describe('ActivityPanel', () => {
  let mockTogglePanel: ReturnType<typeof vi.fn>;
  let mockSetPanelWidth: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    vi.clearAllMocks();

    mockTogglePanel = vi.fn();
    mockSetPanelWidth = vi.fn();

    // Default useChatStore mock - no current message
    (useChatStore as unknown as ReturnType<typeof vi.fn>).mockImplementation((selector) => {
      if (typeof selector === 'function') {
        return selector({ messages: [] });
      }
      return { messages: [] };
    });
  });

  describe('visibility', () => {
    it('returns null when isPanelVisible is false', () => {
      (useActivityStore as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        isPanelVisible: false,
        panelWidth: 400,
        togglePanel: mockTogglePanel,
        setPanelWidth: mockSetPanelWidth,
        currentMessageId: null,
      });

      const { container } = render(<ActivityPanel />);

      expect(container.firstChild).toBeNull();
    });

    it('renders panel when isPanelVisible is true', () => {
      (useActivityStore as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        isPanelVisible: true,
        panelWidth: 400,
        togglePanel: mockTogglePanel,
        setPanelWidth: mockSetPanelWidth,
        currentMessageId: null,
      });

      render(<ActivityPanel />);

      expect(screen.getByText('Activity')).toBeInTheDocument();
    });
  });

  describe('panel width', () => {
    it('applies the specified panel width', () => {
      (useActivityStore as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        isPanelVisible: true,
        panelWidth: 500,
        togglePanel: mockTogglePanel,
        setPanelWidth: mockSetPanelWidth,
        currentMessageId: null,
      });

      const { container } = render(<ActivityPanel />);
      const panel = container.firstChild as HTMLElement;

      expect(panel.style.width).toBe('500px');
    });
  });

  describe('close button', () => {
    it('renders close button', () => {
      (useActivityStore as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        isPanelVisible: true,
        panelWidth: 400,
        togglePanel: mockTogglePanel,
        setPanelWidth: mockSetPanelWidth,
        currentMessageId: null,
      });

      render(<ActivityPanel />);

      expect(screen.getByTitle('Close activity panel')).toBeInTheDocument();
    });

    it('calls togglePanel when close button is clicked', () => {
      (useActivityStore as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        isPanelVisible: true,
        panelWidth: 400,
        togglePanel: mockTogglePanel,
        setPanelWidth: mockSetPanelWidth,
        currentMessageId: null,
      });

      render(<ActivityPanel />);

      const closeButton = screen.getByTitle('Close activity panel');
      fireEvent.click(closeButton);

      expect(mockTogglePanel).toHaveBeenCalledTimes(1);
    });
  });

  describe('empty state', () => {
    it('shows empty state when there is no activity', () => {
      (useActivityStore as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        isPanelVisible: true,
        panelWidth: 400,
        togglePanel: mockTogglePanel,
        setPanelWidth: mockSetPanelWidth,
        currentMessageId: null,
      });

      render(<ActivityPanel />);

      expect(screen.getByText('No Activity Yet')).toBeInTheDocument();
      expect(screen.getByText('Activity will appear here when an operation starts')).toBeInTheDocument();
    });

    it('shows empty state when current message has no activity data', () => {
      (useActivityStore as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        isPanelVisible: true,
        panelWidth: 400,
        togglePanel: mockTogglePanel,
        setPanelWidth: mockSetPanelWidth,
        currentMessageId: 'msg-123',
      });

      (useChatStore as unknown as ReturnType<typeof vi.fn>).mockImplementation((selector) => {
        if (typeof selector === 'function') {
          return selector({
            messages: [{ id: 'msg-123', plan: null, tasks: [], toolExecutions: [] }],
          });
        }
        return { messages: [] };
      });

      render(<ActivityPanel />);

      expect(screen.getByText('No Activity Yet')).toBeInTheDocument();
    });
  });

  describe('activity content', () => {
    it('renders reasoning section when plan is present', () => {
      const mockPlan = { plan_text: 'Test plan content', reasoning_tokens: 100, timestamp: Date.now() };

      (useActivityStore as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        isPanelVisible: true,
        panelWidth: 400,
        togglePanel: mockTogglePanel,
        setPanelWidth: mockSetPanelWidth,
        currentMessageId: 'msg-123',
      });

      (useChatStore as unknown as ReturnType<typeof vi.fn>).mockImplementation((selector) => {
        if (typeof selector === 'function') {
          return selector({
            messages: [{ id: 'msg-123', plan: mockPlan, tasks: [], toolExecutions: [] }],
          });
        }
        return { messages: [] };
      });

      render(<ActivityPanel />);

      expect(screen.getByTestId('reasoning-section')).toBeInTheDocument();
      expect(screen.getByText(/Test plan content/)).toBeInTheDocument();
    });

    it('renders tasks section when tasks are present', () => {
      const mockTasks = [
        { task_id: 'task-1', sequence: 0, description: 'Task 1', status: 'completed', active_form: 'Doing task 1' },
        { task_id: 'task-2', sequence: 1, description: 'Task 2', status: 'pending', active_form: 'Doing task 2' },
      ];

      (useActivityStore as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        isPanelVisible: true,
        panelWidth: 400,
        togglePanel: mockTogglePanel,
        setPanelWidth: mockSetPanelWidth,
        currentMessageId: 'msg-123',
      });

      (useChatStore as unknown as ReturnType<typeof vi.fn>).mockImplementation((selector) => {
        if (typeof selector === 'function') {
          return selector({
            messages: [{ id: 'msg-123', plan: null, tasks: mockTasks, toolExecutions: [] }],
          });
        }
        return { messages: [] };
      });

      render(<ActivityPanel />);

      expect(screen.getByTestId('tasks-section')).toBeInTheDocument();
      expect(screen.getByText('Tasks: 2')).toBeInTheDocument();
    });

    it('renders tool executions section when toolExecutions are present', () => {
      const mockToolExecutions = [
        { id: 'exec-1', tool: 'read_file', status: 'completed' },
        { id: 'exec-2', tool: 'write_file', status: 'pending' },
        { id: 'exec-3', tool: 'search', status: 'running' },
      ];

      (useActivityStore as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        isPanelVisible: true,
        panelWidth: 400,
        togglePanel: mockTogglePanel,
        setPanelWidth: mockSetPanelWidth,
        currentMessageId: 'msg-123',
      });

      (useChatStore as unknown as ReturnType<typeof vi.fn>).mockImplementation((selector) => {
        if (typeof selector === 'function') {
          return selector({
            messages: [{ id: 'msg-123', plan: null, tasks: [], toolExecutions: mockToolExecutions }],
          });
        }
        return { messages: [] };
      });

      render(<ActivityPanel />);

      expect(screen.getByTestId('tool-executions-section')).toBeInTheDocument();
      expect(screen.getByText('Tools: 3')).toBeInTheDocument();
    });

    it('renders all sections when all activity types are present', () => {
      const mockPlan = { plan_text: 'Test plan', reasoning_tokens: 50, timestamp: Date.now() };
      const mockTasks = [{ task_id: 'task-1', sequence: 0, description: 'Task 1', status: 'completed', active_form: 'Task 1' }];
      const mockToolExecutions = [{ id: 'exec-1', tool: 'read_file', status: 'completed' }];

      (useActivityStore as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        isPanelVisible: true,
        panelWidth: 400,
        togglePanel: mockTogglePanel,
        setPanelWidth: mockSetPanelWidth,
        currentMessageId: 'msg-123',
      });

      (useChatStore as unknown as ReturnType<typeof vi.fn>).mockImplementation((selector) => {
        if (typeof selector === 'function') {
          return selector({
            messages: [{ id: 'msg-123', plan: mockPlan, tasks: mockTasks, toolExecutions: mockToolExecutions }],
          });
        }
        return { messages: [] };
      });

      render(<ActivityPanel />);

      expect(screen.getByTestId('reasoning-section')).toBeInTheDocument();
      expect(screen.getByTestId('tasks-section')).toBeInTheDocument();
      expect(screen.getByTestId('tool-executions-section')).toBeInTheDocument();
    });
  });

  describe('message selection', () => {
    it('displays activity for the selected message', () => {
      const mockTasks = [{ task_id: 'task-1', sequence: 0, description: 'Selected Task', status: 'completed', active_form: 'Task' }];

      (useActivityStore as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        isPanelVisible: true,
        panelWidth: 400,
        togglePanel: mockTogglePanel,
        setPanelWidth: mockSetPanelWidth,
        currentMessageId: 'msg-selected',
      });

      (useChatStore as unknown as ReturnType<typeof vi.fn>).mockImplementation((selector) => {
        if (typeof selector === 'function') {
          return selector({
            messages: [
              { id: 'msg-other', plan: null, tasks: [], toolExecutions: [] },
              { id: 'msg-selected', plan: null, tasks: mockTasks, toolExecutions: [] },
            ],
          });
        }
        return { messages: [] };
      });

      render(<ActivityPanel />);

      expect(screen.getByTestId('tasks-section')).toBeInTheDocument();
    });

    it('shows empty state when selected message is not found', () => {
      (useActivityStore as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        isPanelVisible: true,
        panelWidth: 400,
        togglePanel: mockTogglePanel,
        setPanelWidth: mockSetPanelWidth,
        currentMessageId: 'msg-nonexistent',
      });

      (useChatStore as unknown as ReturnType<typeof vi.fn>).mockImplementation((selector) => {
        if (typeof selector === 'function') {
          return selector({
            messages: [{ id: 'msg-other', plan: null, tasks: [], toolExecutions: [] }],
          });
        }
        return { messages: [] };
      });

      render(<ActivityPanel />);

      expect(screen.getByText('No Activity Yet')).toBeInTheDocument();
    });
  });

  describe('resize handle', () => {
    it('renders resize handle', () => {
      (useActivityStore as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        isPanelVisible: true,
        panelWidth: 400,
        togglePanel: mockTogglePanel,
        setPanelWidth: mockSetPanelWidth,
        currentMessageId: null,
      });

      const { container } = render(<ActivityPanel />);

      const resizeHandle = container.querySelector('.cursor-col-resize');
      expect(resizeHandle).toBeInTheDocument();
    });

    it('triggers resize on mousedown', () => {
      (useActivityStore as unknown as ReturnType<typeof vi.fn>).mockReturnValue({
        isPanelVisible: true,
        panelWidth: 400,
        togglePanel: mockTogglePanel,
        setPanelWidth: mockSetPanelWidth,
        currentMessageId: null,
      });

      const { container } = render(<ActivityPanel />);

      const resizeHandle = container.querySelector('.cursor-col-resize');
      fireEvent.mouseDown(resizeHandle!);

      // Simulate mouse move to trigger resize
      fireEvent.mouseMove(document, { clientX: 800 });

      expect(mockSetPanelWidth).toHaveBeenCalled();
    });
  });
});
