// frontend/src/components/ActivityPanel.tsx
// Real-time activity panel showing reasoning, tasks, and tool executions

import React, { useRef, useEffect, useState } from 'react';
import { useActivityStore } from '../stores/useActivityStore';
import { useChatStore } from '../stores/useChatStore';
import { X, Activity, GripVertical } from 'lucide-react';
import { ReasoningSection } from './ActivitySections/ReasoningSection';
import { TasksSection } from './ActivitySections/TasksSection';
import { ToolExecutionsSection } from './ActivitySections/ToolExecutionsSection';

export function ActivityPanel() {
  const {
    isPanelVisible,
    panelWidth,
    togglePanel,
    setPanelWidth,
    currentMessageId,
  } = useActivityStore();

  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const resizeHandleRef = useRef<HTMLDivElement>(null);
  const [isResizing, setIsResizing] = useState(false);

  // Subscribe to chat store to get reactive updates
  const currentMessage = useChatStore(state =>
    state.messages.find(m => m.id === currentMessageId)
  );

  // Get current activity data from the message
  const plan = currentMessage?.plan;
  const tasks = currentMessage?.tasks || [];
  const toolExecutions = currentMessage?.toolExecutions || [];

  const hasActivity = plan || tasks.length > 0 || toolExecutions.length > 0;

  // Auto-scroll to bottom when new activity arrives
  useEffect(() => {
    if (scrollContainerRef.current && hasActivity) {
      scrollContainerRef.current.scrollTop = scrollContainerRef.current.scrollHeight;
    }
  }, [tasks.length, toolExecutions.length, hasActivity]);

  // Handle panel resize
  useEffect(() => {
    if (!isResizing) return;

    const handleMouseMove = (e: MouseEvent) => {
      const newWidth = window.innerWidth - e.clientX;
      setPanelWidth(newWidth);
    };

    const handleMouseUp = () => {
      setIsResizing(false);
    };

    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);

    return () => {
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
    };
  }, [isResizing, setPanelWidth]);

  if (!isPanelVisible) {
    return null;
  }

  return (
    <div
      className="flex-shrink-0 bg-gray-50 dark:bg-slate-900 border-l border-gray-200 dark:border-slate-700 flex relative"
      style={{ width: `${panelWidth}px` }}
    >
      {/* Resize Handle */}
      <div
        ref={resizeHandleRef}
        onMouseDown={() => setIsResizing(true)}
        className="absolute left-0 top-0 bottom-0 w-1 cursor-col-resize hover:bg-blue-500/50 transition-colors group"
      >
        <div className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 opacity-0 group-hover:opacity-100 transition-opacity">
          <GripVertical className="w-4 h-4 text-gray-400 dark:text-slate-400" />
        </div>
      </div>

      {/* Panel Content */}
      <div className="flex-1 flex flex-col ml-1">
        {/* Header */}
        <div className="flex-shrink-0 flex items-center justify-between px-4 py-3 border-b border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-850">
          <div className="flex items-center gap-2">
            <Activity className="w-4 h-4 text-blue-500 dark:text-blue-400" />
            <h2 className="text-sm font-semibold text-gray-800 dark:text-slate-200">Activity</h2>
          </div>
          <button
            onClick={togglePanel}
            className="p-1 hover:bg-gray-100 dark:hover:bg-slate-700 rounded transition-colors"
            title="Close activity panel"
          >
            <X className="w-4 h-4 text-gray-500 dark:text-slate-400" />
          </button>
        </div>

        {/* Scrollable Content */}
        <div
          ref={scrollContainerRef}
          className="flex-1 overflow-y-auto scrollbar-thin scrollbar-thumb-gray-300 dark:scrollbar-thumb-slate-700 scrollbar-track-gray-100 dark:scrollbar-track-slate-900"
        >
          {hasActivity ? (
            <>
              {/* Reasoning Section */}
              <ReasoningSection plan={plan} />

              {/* Tasks Section */}
              <TasksSection tasks={tasks} />

              {/* Tool Executions Section */}
              <ToolExecutionsSection toolExecutions={toolExecutions} />
            </>
          ) : (
            /* Empty State */
            <div className="flex flex-col items-center justify-center h-full text-center px-6 py-12">
              <Activity className="w-12 h-12 text-gray-400 dark:text-slate-600 mb-4" />
              <h3 className="text-sm font-medium text-gray-500 dark:text-slate-400 mb-2">No Activity Yet</h3>
              <p className="text-xs text-gray-400 dark:text-slate-500">
                Activity will appear here when an operation starts
              </p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
