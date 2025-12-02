// src/components/__tests__/TaskTracker.test.tsx
// TaskTracker Component Tests

import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { TaskTracker } from '../TaskTracker';
import { Task } from '../../stores/useChatStore';

// Mock TaskItem component to simplify testing
vi.mock('../TaskItem', () => ({
  TaskItem: ({ task }: { task: Task }) => (
    <div data-testid={`task-item-${task.task_id}`}>
      <span data-testid="task-description">{task.description}</span>
      <span data-testid="task-status">{task.status}</span>
    </div>
  ),
}));

const createTask = (overrides: Partial<Task> = {}): Task => ({
  task_id: 'task-1',
  sequence: 0,
  description: 'Test task description',
  active_form: 'Testing task',
  status: 'pending',
  ...overrides,
});

describe('TaskTracker', () => {
  describe('rendering', () => {
    it('returns null when tasks array is empty', () => {
      const { container } = render(<TaskTracker tasks={[]} />);
      expect(container.firstChild).toBeNull();
    });

    it('returns null when tasks is undefined', () => {
      const { container } = render(<TaskTracker tasks={undefined as any} />);
      expect(container.firstChild).toBeNull();
    });

    it('renders when tasks are provided', () => {
      const tasks: Task[] = [createTask()];
      render(<TaskTracker tasks={tasks} />);

      expect(screen.getByText('Task Progress')).toBeInTheDocument();
    });

    it('renders all tasks', () => {
      const tasks: Task[] = [
        createTask({ task_id: 'task-1', description: 'First task' }),
        createTask({ task_id: 'task-2', description: 'Second task', sequence: 1 }),
        createTask({ task_id: 'task-3', description: 'Third task', sequence: 2 }),
      ];

      render(<TaskTracker tasks={tasks} />);

      expect(screen.getByTestId('task-item-task-1')).toBeInTheDocument();
      expect(screen.getByTestId('task-item-task-2')).toBeInTheDocument();
      expect(screen.getByTestId('task-item-task-3')).toBeInTheDocument();
    });
  });

  describe('progress calculation', () => {
    it('shows completed count out of total', () => {
      const tasks: Task[] = [
        createTask({ task_id: 'task-1', status: 'completed' }),
        createTask({ task_id: 'task-2', status: 'completed', sequence: 1 }),
        createTask({ task_id: 'task-3', status: 'pending', sequence: 2 }),
      ];

      render(<TaskTracker tasks={tasks} />);

      expect(screen.getByText('2 / 3 completed')).toBeInTheDocument();
    });

    it('shows running count when tasks are running', () => {
      const tasks: Task[] = [
        createTask({ task_id: 'task-1', status: 'completed' }),
        createTask({ task_id: 'task-2', status: 'running', sequence: 1 }),
        createTask({ task_id: 'task-3', status: 'running', sequence: 2 }),
      ];

      render(<TaskTracker tasks={tasks} />);

      expect(screen.getByText('2 running')).toBeInTheDocument();
    });

    it('shows failed count when tasks have failed', () => {
      const tasks: Task[] = [
        createTask({ task_id: 'task-1', status: 'completed' }),
        createTask({ task_id: 'task-2', status: 'failed', sequence: 1 }),
      ];

      render(<TaskTracker tasks={tasks} />);

      expect(screen.getByText('1 failed')).toBeInTheDocument();
    });

    it('does not show running count when no tasks are running', () => {
      const tasks: Task[] = [
        createTask({ task_id: 'task-1', status: 'completed' }),
        createTask({ task_id: 'task-2', status: 'pending', sequence: 1 }),
      ];

      render(<TaskTracker tasks={tasks} />);

      expect(screen.queryByText(/running/)).not.toBeInTheDocument();
    });

    it('does not show failed count when no tasks have failed', () => {
      const tasks: Task[] = [
        createTask({ task_id: 'task-1', status: 'completed' }),
        createTask({ task_id: 'task-2', status: 'pending', sequence: 1 }),
      ];

      render(<TaskTracker tasks={tasks} />);

      expect(screen.queryByText(/failed/)).not.toBeInTheDocument();
    });

    it('shows all counts together when applicable', () => {
      const tasks: Task[] = [
        createTask({ task_id: 'task-1', status: 'completed' }),
        createTask({ task_id: 'task-2', status: 'completed', sequence: 1 }),
        createTask({ task_id: 'task-3', status: 'running', sequence: 2 }),
        createTask({ task_id: 'task-4', status: 'failed', sequence: 3 }),
        createTask({ task_id: 'task-5', status: 'pending', sequence: 4 }),
      ];

      render(<TaskTracker tasks={tasks} />);

      expect(screen.getByText('1 running')).toBeInTheDocument();
      expect(screen.getByText('2 / 5 completed')).toBeInTheDocument();
      expect(screen.getByText('1 failed')).toBeInTheDocument();
    });
  });

  describe('with operationId', () => {
    it('accepts optional operationId prop', () => {
      const tasks: Task[] = [createTask()];

      // Should not throw
      render(<TaskTracker tasks={tasks} operationId="op-123" />);

      expect(screen.getByText('Task Progress')).toBeInTheDocument();
    });
  });

  describe('status combinations', () => {
    it('handles all pending tasks', () => {
      const tasks: Task[] = [
        createTask({ task_id: 'task-1', status: 'pending' }),
        createTask({ task_id: 'task-2', status: 'pending', sequence: 1 }),
      ];

      render(<TaskTracker tasks={tasks} />);

      expect(screen.getByText('0 / 2 completed')).toBeInTheDocument();
    });

    it('handles all completed tasks', () => {
      const tasks: Task[] = [
        createTask({ task_id: 'task-1', status: 'completed' }),
        createTask({ task_id: 'task-2', status: 'completed', sequence: 1 }),
      ];

      render(<TaskTracker tasks={tasks} />);

      expect(screen.getByText('2 / 2 completed')).toBeInTheDocument();
    });

    it('handles all failed tasks', () => {
      const tasks: Task[] = [
        createTask({ task_id: 'task-1', status: 'failed' }),
        createTask({ task_id: 'task-2', status: 'failed', sequence: 1 }),
      ];

      render(<TaskTracker tasks={tasks} />);

      expect(screen.getByText('0 / 2 completed')).toBeInTheDocument();
      expect(screen.getByText('2 failed')).toBeInTheDocument();
    });

    it('handles single task', () => {
      const tasks: Task[] = [createTask({ status: 'running' })];

      render(<TaskTracker tasks={tasks} />);

      expect(screen.getByText('1 running')).toBeInTheDocument();
      expect(screen.getByText('0 / 1 completed')).toBeInTheDocument();
    });
  });
});
