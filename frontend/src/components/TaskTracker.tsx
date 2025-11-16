// src/components/TaskTracker.tsx
// Display list of tasks with real-time status updates

import React from 'react';
import { ListChecks } from 'lucide-react';
import { Task } from '../stores/useChatStore';
import { TaskItem } from './TaskItem';

interface TaskTrackerProps {
  tasks: Task[];
  operationId?: string;
}

export const TaskTracker: React.FC<TaskTrackerProps> = ({ tasks, operationId }) => {
  if (!tasks || tasks.length === 0) {
    return null;
  }

  // Calculate progress
  const completedCount = tasks.filter(t => t.status === 'completed').length;
  const failedCount = tasks.filter(t => t.status === 'failed').length;
  const runningCount = tasks.filter(t => t.status === 'running').length;
  const totalCount = tasks.length;

  return (
    <div className="mt-4 border-t border-gray-700 pt-3">
      {/* Header with Progress */}
      <div className="flex items-center justify-between mb-3">
        <div className="flex items-center gap-2 text-sm text-gray-400">
          <ListChecks className="w-4 h-4" />
          <span>Task Progress</span>
        </div>
        <div className="flex items-center gap-3 text-xs text-gray-500">
          {runningCount > 0 && (
            <span className="text-blue-400">
              {runningCount} running
            </span>
          )}
          <span>
            {completedCount} / {totalCount} completed
          </span>
          {failedCount > 0 && (
            <span className="text-red-400">
              {failedCount} failed
            </span>
          )}
        </div>
      </div>

      {/* Task List */}
      <div className="space-y-2">
        {tasks.map((task) => (
          <TaskItem key={task.task_id} task={task} />
        ))}
      </div>
    </div>
  );
};
