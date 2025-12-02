// frontend/src/stores/__tests__/activityStore.test.ts
// Activity Store Tests

import { describe, it, expect, beforeEach, vi } from 'vitest';
import { useActivityStore } from '../useActivityStore';
import { useChatStore } from '../useChatStore';

// Mock useChatStore
vi.mock('../useChatStore', () => ({
  useChatStore: {
    getState: vi.fn(() => ({
      messages: [],
    })),
  },
}));

describe('useActivityStore', () => {
  beforeEach(() => {
    // Reset store state before each test
    useActivityStore.setState({
      isPanelVisible: true,
      panelWidth: 400,
      currentOperationId: null,
      currentMessageId: null,
    });
    vi.clearAllMocks();
  });

  describe('initial state', () => {
    it('should have correct initial values', () => {
      const state = useActivityStore.getState();

      expect(state.isPanelVisible).toBe(true);
      expect(state.panelWidth).toBe(400);
      expect(state.currentOperationId).toBeNull();
      expect(state.currentMessageId).toBeNull();
    });
  });

  describe('panel controls', () => {
    it('should toggle panel visibility', () => {
      expect(useActivityStore.getState().isPanelVisible).toBe(true);

      useActivityStore.getState().togglePanel();
      expect(useActivityStore.getState().isPanelVisible).toBe(false);

      useActivityStore.getState().togglePanel();
      expect(useActivityStore.getState().isPanelVisible).toBe(true);
    });

    it('should show panel', () => {
      useActivityStore.setState({ isPanelVisible: false });

      useActivityStore.getState().showPanel();

      expect(useActivityStore.getState().isPanelVisible).toBe(true);
    });

    it('should hide panel', () => {
      useActivityStore.setState({ isPanelVisible: true });

      useActivityStore.getState().hidePanel();

      expect(useActivityStore.getState().isPanelVisible).toBe(false);
    });

    it('should set panel width with min/max constraints', () => {
      // Normal width
      useActivityStore.getState().setPanelWidth(500);
      expect(useActivityStore.getState().panelWidth).toBe(500);

      // Below minimum (300)
      useActivityStore.getState().setPanelWidth(200);
      expect(useActivityStore.getState().panelWidth).toBe(300);

      // Above maximum (800)
      useActivityStore.getState().setPanelWidth(1000);
      expect(useActivityStore.getState().panelWidth).toBe(800);
    });
  });

  describe('operation tracking', () => {
    it('should set current operation', () => {
      useActivityStore.getState().setCurrentOperation('op-123', 'msg-456');

      expect(useActivityStore.getState().currentOperationId).toBe('op-123');
      expect(useActivityStore.getState().currentMessageId).toBe('msg-456');
    });

    it('should clear current operation', () => {
      useActivityStore.setState({
        currentOperationId: 'op-123',
        currentMessageId: 'msg-456',
      });

      useActivityStore.getState().clearCurrentOperation();

      expect(useActivityStore.getState().currentOperationId).toBeNull();
      expect(useActivityStore.getState().currentMessageId).toBeNull();
    });
  });

  describe('data accessors', () => {
    it('should return undefined plan when no message selected', () => {
      const plan = useActivityStore.getState().getCurrentPlan();
      expect(plan).toBeUndefined();
    });

    it('should return empty tasks when no message selected', () => {
      const tasks = useActivityStore.getState().getCurrentTasks();
      expect(tasks).toEqual([]);
    });

    it('should return empty tool executions when no message selected', () => {
      const executions = useActivityStore.getState().getCurrentToolExecutions();
      expect(executions).toEqual([]);
    });

    it('should get plan from chat store message', () => {
      const mockPlan = {
        id: 'plan-1',
        summary: 'Test plan',
        steps: ['Step 1', 'Step 2'],
        status: 'in_progress' as const,
        timestamp: Date.now(),
      };

      (useChatStore.getState as ReturnType<typeof vi.fn>).mockReturnValue({
        messages: [
          {
            id: 'msg-123',
            plan: mockPlan,
            tasks: [],
            toolExecutions: [],
          },
        ],
      });

      useActivityStore.setState({ currentMessageId: 'msg-123' });

      const plan = useActivityStore.getState().getCurrentPlan();
      expect(plan).toEqual(mockPlan);
    });

    it('should get tasks from chat store message', () => {
      const mockTasks = [
        { id: 'task-1', description: 'Task 1', status: 'completed' as const, timestamp: 1000 },
        { id: 'task-2', description: 'Task 2', status: 'in_progress' as const, timestamp: 2000 },
      ];

      (useChatStore.getState as ReturnType<typeof vi.fn>).mockReturnValue({
        messages: [
          {
            id: 'msg-123',
            tasks: mockTasks,
            toolExecutions: [],
          },
        ],
      });

      useActivityStore.setState({ currentMessageId: 'msg-123' });

      const tasks = useActivityStore.getState().getCurrentTasks();
      expect(tasks).toEqual(mockTasks);
    });

    it('should get tool executions from chat store message', () => {
      const mockExecutions = [
        { id: 'exec-1', tool: 'read_file', args: { path: '/test.ts' }, result: 'content', status: 'completed' as const, timestamp: 1000 },
        { id: 'exec-2', tool: 'write_file', args: { path: '/out.ts' }, status: 'in_progress' as const, timestamp: 2000 },
      ];

      (useChatStore.getState as ReturnType<typeof vi.fn>).mockReturnValue({
        messages: [
          {
            id: 'msg-123',
            tasks: [],
            toolExecutions: mockExecutions,
          },
        ],
      });

      useActivityStore.setState({ currentMessageId: 'msg-123' });

      const executions = useActivityStore.getState().getCurrentToolExecutions();
      expect(executions).toEqual(mockExecutions);
    });
  });

  describe('getAllActivity', () => {
    it('should return empty array when no message selected', () => {
      const activities = useActivityStore.getState().getAllActivity();
      expect(activities).toEqual([]);
    });

    it('should aggregate and sort all activity by timestamp', () => {
      const mockPlan = {
        id: 'plan-1',
        summary: 'Test plan',
        steps: ['Step 1'],
        status: 'completed' as const,
        timestamp: 1000,
      };

      const mockTasks = [
        { id: 'task-1', description: 'Task 1', status: 'completed' as const, timestamp: 2000 },
      ];

      const mockExecutions = [
        { id: 'exec-1', tool: 'read_file', args: {}, status: 'completed' as const, timestamp: 1500 },
      ];

      (useChatStore.getState as ReturnType<typeof vi.fn>).mockReturnValue({
        messages: [
          {
            id: 'msg-123',
            plan: mockPlan,
            tasks: mockTasks,
            toolExecutions: mockExecutions,
          },
        ],
      });

      useActivityStore.setState({ currentMessageId: 'msg-123' });

      const activities = useActivityStore.getState().getAllActivity();

      expect(activities).toHaveLength(3);
      // Should be sorted by timestamp: plan(1000), exec(1500), task(2000)
      expect(activities[0].type).toBe('plan');
      expect(activities[0].timestamp).toBe(1000);
      expect(activities[1].type).toBe('tool');
      expect(activities[1].timestamp).toBe(1500);
      expect(activities[2].type).toBe('task');
      expect(activities[2].timestamp).toBe(2000);
    });
  });
});
