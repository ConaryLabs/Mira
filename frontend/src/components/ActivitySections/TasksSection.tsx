// frontend/src/components/ActivitySections/TasksSection.tsx
// Displays task-by-task progress with status indicators

import React, { useState } from 'react';
import { Task } from '../../stores/useChatStore';
import { TaskItem } from '../TaskItem';
import { ChevronDown, ChevronRight, ListChecks } from 'lucide-react';

interface TasksSectionProps {
  tasks: Task[];
}

export function TasksSection({ tasks }: TasksSectionProps) {
  const [isExpanded, setIsExpanded] = useState(true);

  if (tasks.length === 0) {
    return null;
  }

  // Count tasks by status
  const statusCounts = tasks.reduce((acc, task) => {
    acc[task.status] = (acc[task.status] || 0) + 1;
    return acc;
  }, {} as Record<string, number>);

  const completedCount = statusCounts.completed || 0;
  const totalCount = tasks.length;

  return (
    <div className="border-b border-slate-700">
      {/* Section Header */}
      <button
        onClick={() => setIsExpanded(!isExpanded)}
        className="w-full flex items-center justify-between px-4 py-3 hover:bg-slate-800/50 transition-colors"
      >
        <div className="flex items-center gap-2">
          {isExpanded ? (
            <ChevronDown className="w-4 h-4 text-slate-400" />
          ) : (
            <ChevronRight className="w-4 h-4 text-slate-400" />
          )}
          <ListChecks className="w-4 h-4 text-green-400" />
          <span className="text-sm font-medium text-slate-200">Tasks</span>
        </div>
        <div className="text-xs text-slate-400">
          {completedCount}/{totalCount} completed
        </div>
      </button>

      {/* Section Content */}
      {isExpanded && (
        <div className="px-4 pb-4 space-y-2">
          {tasks.map((task) => (
            <TaskItem key={task.task_id} task={task} />
          ))}
        </div>
      )}
    </div>
  );
}
