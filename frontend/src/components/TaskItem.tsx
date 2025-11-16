// src/components/TaskItem.tsx
// Display individual task with status indicator and animation

import React from 'react';
import { Clock, Loader, CheckCircle, XCircle } from 'lucide-react';
import { Task, TaskStatus } from '../stores/useChatStore';

interface TaskItemProps {
  task: Task;
}

const statusConfig: Record<TaskStatus, {
  icon: React.ElementType;
  color: string;
  bgColor: string;
  animate?: string;
}> = {
  pending: {
    icon: Clock,
    color: 'text-gray-400',
    bgColor: 'bg-gray-800',
  },
  running: {
    icon: Loader,
    color: 'text-blue-400',
    bgColor: 'bg-blue-900/20',
    animate: 'animate-spin',
  },
  completed: {
    icon: CheckCircle,
    color: 'text-green-400',
    bgColor: 'bg-green-900/20',
  },
  failed: {
    icon: XCircle,
    color: 'text-red-400',
    bgColor: 'bg-red-900/20',
  },
};

export const TaskItem: React.FC<TaskItemProps> = ({ task }) => {
  const config = statusConfig[task.status];
  const Icon = config.icon;

  return (
    <div className={`flex items-start gap-3 p-3 rounded-lg ${config.bgColor} transition-colors duration-300`}>
      {/* Status Icon */}
      <div className="flex-shrink-0 mt-0.5">
        <Icon className={`w-4 h-4 ${config.color} ${config.animate || ''}`} />
      </div>

      {/* Task Content */}
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          {/* Sequence Number */}
          <span className="text-xs font-medium text-gray-500">
            {task.sequence + 1}.
          </span>

          {/* Description or Active Form */}
          <span className={`text-sm ${task.status === 'running' ? 'font-medium' : ''} ${config.color}`}>
            {task.status === 'running' ? task.active_form : task.description}
          </span>
        </div>

        {/* Error Message (if failed) */}
        {task.status === 'failed' && task.error && (
          <div className="mt-1 text-xs text-red-400/80">
            {task.error}
          </div>
        )}
      </div>
    </div>
  );
};
