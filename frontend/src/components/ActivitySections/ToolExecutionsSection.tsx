// frontend/src/components/ActivitySections/ToolExecutionsSection.tsx
// Displays tool execution log with success/failure indicators

import React, { useState } from 'react';
import { ToolExecution } from '../../stores/useChatStore';
import { ChevronDown, ChevronRight, Wrench, CheckCircle, XCircle, FileText, GitBranch, Code, Terminal } from 'lucide-react';

interface ToolExecutionsSectionProps {
  toolExecutions: ToolExecution[];
}

// Map tool types to icons
const toolIconMap: Record<string, React.ElementType> = {
  file: FileText,
  git: GitBranch,
  code: Code,
  command: Terminal,
  default: Wrench,
};

function getToolIcon(toolType: string): React.ElementType {
  return toolIconMap[toolType.toLowerCase()] || toolIconMap.default;
}

// Format timestamp to relative time
function formatTimestamp(timestamp: number): string {
  const now = Date.now();
  const diff = now - timestamp;

  if (diff < 1000) return 'just now';
  if (diff < 60000) return `${Math.floor(diff / 1000)}s ago`;
  if (diff < 3600000) return `${Math.floor(diff / 60000)}m ago`;
  return new Date(timestamp).toLocaleTimeString();
}

export function ToolExecutionsSection({ toolExecutions }: ToolExecutionsSectionProps) {
  const [isExpanded, setIsExpanded] = useState(true);
  const [expandedItems, setExpandedItems] = useState<Set<number>>(new Set());

  if (toolExecutions.length === 0) {
    return null;
  }

  const toggleItemExpand = (index: number) => {
    const newExpanded = new Set(expandedItems);
    if (newExpanded.has(index)) {
      newExpanded.delete(index);
    } else {
      newExpanded.add(index);
    }
    setExpandedItems(newExpanded);
  };

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
          <Wrench className="w-4 h-4 text-purple-400" />
          <span className="text-sm font-medium text-slate-200">Tool Executions</span>
        </div>
        <div className="text-xs text-slate-400">
          {toolExecutions.length} {toolExecutions.length === 1 ? 'call' : 'calls'}
        </div>
      </button>

      {/* Section Content */}
      {isExpanded && (
        <div className="px-4 pb-4 space-y-2">
          {toolExecutions.map((execution, index) => {
            const ToolIcon = getToolIcon(execution.toolType);
            const isItemExpanded = expandedItems.has(index);
            const hasDetails = execution.details && Object.keys(execution.details).length > 0;

            return (
              <div
                key={index}
                className={`
                  rounded-lg border transition-colors
                  ${execution.success
                    ? 'bg-green-900/10 border-green-800/30'
                    : 'bg-red-900/10 border-red-800/30'
                  }
                `}
              >
                {/* Main Row */}
                <button
                  onClick={() => hasDetails && toggleItemExpand(index)}
                  className={`
                    w-full flex items-start gap-3 p-3
                    ${hasDetails ? 'cursor-pointer hover:bg-slate-800/30' : 'cursor-default'}
                  `}
                >
                  {/* Tool Icon */}
                  <div className="flex-shrink-0 mt-0.5">
                    <ToolIcon className={`w-4 h-4 ${execution.success ? 'text-green-400' : 'text-red-400'}`} />
                  </div>

                  {/* Content */}
                  <div className="flex-1 min-w-0 text-left">
                    {/* Tool Name & Summary */}
                    <div className="flex items-center gap-2 flex-wrap">
                      <span className="text-xs font-mono text-slate-400">
                        {execution.toolName}
                      </span>
                      <span className="text-sm text-slate-300">
                        {execution.summary}
                      </span>
                    </div>

                    {/* Timestamp */}
                    <div className="text-xs text-slate-500 mt-1">
                      {formatTimestamp(execution.timestamp)}
                    </div>
                  </div>

                  {/* Success/Failure Icon */}
                  <div className="flex-shrink-0">
                    {execution.success ? (
                      <CheckCircle className="w-4 h-4 text-green-400" />
                    ) : (
                      <XCircle className="w-4 h-4 text-red-400" />
                    )}
                  </div>
                </button>

                {/* Expanded Details */}
                {isItemExpanded && hasDetails && (
                  <div className="px-3 pb-3 mt-2 border-t border-slate-700/50">
                    <div className="bg-slate-900/50 rounded p-2 mt-2">
                      <pre className="text-xs text-slate-400 font-mono overflow-x-auto">
                        {JSON.stringify(execution.details, null, 2)}
                      </pre>
                    </div>
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
