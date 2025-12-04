// src/components/ThinkingIndicator.tsx
import React from 'react';
import { Bot } from 'lucide-react';

export const ThinkingIndicator: React.FC = () => {
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
          <span className="text-xs text-gray-500 dark:text-slate-500">thinking...</span>
        </div>

        {/* Animated dots */}
        <div className="flex items-center gap-1 text-gray-400 dark:text-slate-400">
          <div className="flex items-center gap-1">
            <div className="w-2 h-2 bg-gray-400 dark:bg-slate-500 rounded-full animate-pulse" style={{ animationDelay: '0ms' }}></div>
            <div className="w-2 h-2 bg-gray-400 dark:bg-slate-500 rounded-full animate-pulse" style={{ animationDelay: '150ms' }}></div>
            <div className="w-2 h-2 bg-gray-400 dark:bg-slate-500 rounded-full animate-pulse" style={{ animationDelay: '300ms' }}></div>
          </div>
        </div>
      </div>
    </div>
  );
};
