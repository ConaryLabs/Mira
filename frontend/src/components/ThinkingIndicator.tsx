// src/components/ThinkingIndicator.tsx
import React from 'react';
import { Bot, Brain, Wrench, Search } from 'lucide-react';
import { useUsageStore } from '../stores/useUsageStore';

interface ThinkingIndicatorProps {
  showTokens?: boolean;
}

export const ThinkingIndicator: React.FC<ThinkingIndicatorProps> = ({ showTokens = true }) => {
  const thinkingStatus = useUsageStore(state => state.thinkingStatus);

  // Don't render if no thinking status
  if (!thinkingStatus) {
    return null;
  }

  const { status, message, tokensIn, tokensOut, activeTool } = thinkingStatus;
  const totalTokens = tokensIn + tokensOut;

  // Get icon based on status
  const getStatusIcon = () => {
    switch (status) {
      case 'gathering_context':
        return <Search size={14} className="text-blue-400" />;
      case 'executing_tool':
        return <Wrench size={14} className="text-amber-400" />;
      case 'thinking':
      default:
        return <Brain size={14} className="text-purple-400" />;
    }
  };

  // Format token count
  const formatTokens = (count: number) => {
    if (count >= 1000000) {
      return `${(count / 1000000).toFixed(1)}M`;
    } else if (count >= 1000) {
      return `${(count / 1000).toFixed(1)}K`;
    }
    return count.toString();
  };

  return (
    <div className="flex gap-3 group">
      {/* Avatar */}
      <div className="w-8 h-8 bg-gradient-to-br from-purple-500 to-pink-500 rounded-full flex items-center justify-center flex-shrink-0">
        <Bot size={16} className="text-white" />
      </div>

      {/* Thinking content */}
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2 mb-1">
          <span className="font-medium text-sm text-gray-900 dark:text-slate-100">Mira</span>
          <div className="flex items-center gap-1.5 text-xs text-gray-500 dark:text-slate-500">
            {getStatusIcon()}
            <span>{message || 'Thinking...'}</span>
            {activeTool && (
              <span className="text-amber-500 dark:text-amber-400">({activeTool})</span>
            )}
          </div>
        </div>

        {/* Status bar with token count */}
        <div className="flex items-center gap-3 text-gray-400 dark:text-slate-400">
          {/* Animated dots */}
          <div className="flex items-center gap-1">
            <div className="w-2 h-2 bg-purple-400 dark:bg-purple-500 rounded-full animate-pulse" style={{ animationDelay: '0ms' }}></div>
            <div className="w-2 h-2 bg-purple-400 dark:bg-purple-500 rounded-full animate-pulse" style={{ animationDelay: '150ms' }}></div>
            <div className="w-2 h-2 bg-purple-400 dark:bg-purple-500 rounded-full animate-pulse" style={{ animationDelay: '300ms' }}></div>
          </div>

          {/* Token count */}
          {showTokens && totalTokens > 0 && (
            <div className="flex items-center gap-1 text-xs text-gray-500 dark:text-slate-500 font-mono">
              <span className="text-green-500">{formatTokens(tokensIn)}</span>
              <span>/</span>
              <span className="text-blue-500">{formatTokens(tokensOut)}</span>
              <span className="text-gray-400 dark:text-slate-600 ml-1">tokens</span>
            </div>
          )}
        </div>
      </div>
    </div>
  );
};

// Compact version for status bar or header
export const ThinkingIndicatorCompact: React.FC = () => {
  const thinkingStatus = useUsageStore(state => state.thinkingStatus);

  if (!thinkingStatus) {
    return null;
  }

  const { message, tokensIn, tokensOut, activeTool } = thinkingStatus;
  const totalTokens = tokensIn + tokensOut;

  const formatTokens = (count: number) => {
    if (count >= 1000) {
      return `${(count / 1000).toFixed(1)}K`;
    }
    return count.toString();
  };

  return (
    <div className="flex items-center gap-2 text-xs text-gray-500 dark:text-slate-400 animate-pulse">
      <Brain size={12} className="text-purple-400" />
      <span>{message || 'Thinking...'}</span>
      {activeTool && <span className="text-amber-400">({activeTool})</span>}
      {totalTokens > 0 && (
        <span className="font-mono text-gray-400 dark:text-slate-500">
          [{formatTokens(totalTokens)} tokens]
        </span>
      )}
    </div>
  );
};
