// src/components/MessageBubble.tsx
// PERFORMANCE FIX: Memoized to prevent re-renders when parent updates

import React from 'react';
import { User, Bot, Copy, ThumbsUp, ThumbsDown, Settings } from 'lucide-react';
import { CodeBlock } from './CodeBlock';
import type { ChatMessage } from '../stores/useChatStore';

interface MessageBubbleProps {
  message: ChatMessage;
  isLast?: boolean;
}

interface CodeBlockMatch {
  language: string;
  code: string;
  fullMatch: string;
}

const MessageBubbleComponent: React.FC<MessageBubbleProps> = ({ message, isLast }) => {
  const handleCopy = () => {
    navigator.clipboard.writeText(message.content);
  };

  const getAvatar = () => {
    switch (message.role) {
      case 'user':
        return (
          <div className="w-8 h-8 bg-blue-600 rounded-full flex items-center justify-center flex-shrink-0">
            <User size={16} className="text-white" />
          </div>
        );
      case 'assistant':
        return (
          <div className="w-8 h-8 bg-gradient-to-br from-purple-500 to-pink-500 rounded-full flex items-center justify-center flex-shrink-0">
            <Bot size={16} className="text-white" />
          </div>
        );
      case 'system':
        return (
          <div className="w-8 h-8 bg-gray-500 rounded-full flex items-center justify-center flex-shrink-0">
            <Settings size={16} className="text-white" />
          </div>
        );
      default:
        return null;
    }
  };

  // Parse content for code blocks and regular text
  const parseContent = (content: string) => {
    const parts: (string | CodeBlockMatch)[] = [];
    
    // Regex to match code blocks with language or without
    const codeBlockRegex = /```(\w+)?\n?([\s\S]*?)```/g;
    
    let lastIndex = 0;
    let match;
    
    while ((match = codeBlockRegex.exec(content)) !== null) {
      // Add text before the code block
      if (match.index > lastIndex) {
        parts.push(content.slice(lastIndex, match.index));
      }
      
      // Add the code block
      parts.push({
        language: match[1] || 'text',
        code: match[2].trim(),
        fullMatch: match[0]
      });
      
      lastIndex = match.index + match[0].length;
    }
    
    // Add remaining text after last code block
    if (lastIndex < content.length) {
      parts.push(content.slice(lastIndex));
    }
    
    // If no code blocks found, return the whole content as text
    if (parts.length === 0) {
      parts.push(content);
    }
    
    return parts;
  };

  // Format regular text content (preserve line breaks and indentation)
  const formatTextContent = (text: string) => {
    if (!text.trim()) return null;
    
    return (
      <div className="whitespace-pre-wrap font-sans">
        {text}
      </div>
    );
  };

  const renderContent = () => {
    const parts = parseContent(message.content);
    
    return (
      <div className="space-y-3">
        {parts.map((part, index) => {
          if (typeof part === 'string') {
            return (
              <div key={index}>
                {formatTextContent(part)}
              </div>
            );
          } else {
            // It's a code block
            return (
              <CodeBlock
                key={index}
                code={part.code}
                language={part.language}
                isDark={true} // Assuming dark theme
              />
            );
          }
        })}
      </div>
    );
  };

  const getMessageStyle = () => {
    switch (message.role) {
      case 'user':
        return 'ml-12'; // Indent user messages
      case 'system':
        return 'bg-gray-50 dark:bg-gray-800/50 rounded-lg p-3 text-sm text-gray-600 dark:text-gray-400 italic';
      default:
        return '';
    }
  };

  if (message.role === 'system') {
    return (
      <div className="bg-slate-800/50 rounded-lg p-3 text-sm text-slate-400 italic">
        <div className="whitespace-pre-wrap">
          {message.content}
        </div>
      </div>
    );
  }

  return (
    <div className={`flex gap-3 group ${getMessageStyle()}`}>
      {/* Avatar */}
      {message.role !== 'user' && getAvatar()}
      
      {/* Message content */}
      <div className="flex-1 min-w-0">
        {/* Header with role and timestamp */}
        <div className="flex items-center gap-2 mb-1">
          <span className="font-medium text-sm text-slate-100">
            {message.role === 'user' ? 'You' : 'Mira'}
          </span>
          <span className="text-xs text-slate-500">
            {new Date(message.timestamp).toLocaleTimeString()}
          </span>
        </div>

        {/* Message body - with proper code formatting */}
        <div className="text-slate-200">
          {renderContent()}
        </div>

        {/* Action buttons (only for assistant messages) */}
        {message.role === 'assistant' && (
          <div className="flex items-center gap-2 mt-3 opacity-0 group-hover:opacity-100 transition-opacity">
            <button
              onClick={handleCopy}
              className="p-1.5 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 rounded-md hover:bg-gray-100 dark:hover:bg-gray-800"
              title="Copy message"
            >
              <Copy size={14} />
            </button>
            <button
              className="p-1.5 text-gray-400 hover:text-green-600 dark:hover:text-green-400 rounded-md hover:bg-gray-100 dark:hover:bg-gray-800"
              title="Good response"
            >
              <ThumbsUp size={14} />
            </button>
            <button
              className="p-1.5 text-gray-400 hover:text-red-600 dark:hover:text-red-400 rounded-md hover:bg-gray-100 dark:hover:bg-gray-800"
              title="Poor response"
            >
              <ThumbsDown size={14} />
            </button>
          </div>
        )}
      </div>

      {/* User avatar on the right */}
      {message.role === 'user' && (
        <div className="flex-shrink-0">
          {getAvatar()}
        </div>
      )}
    </div>
  );
};

// Memoize to prevent unnecessary re-renders when typing in ChatInput
export const MessageBubble = React.memo(MessageBubbleComponent, (prevProps, nextProps) => {
  // Only re-render if message content actually changed
  return (
    prevProps.message.id === nextProps.message.id &&
    prevProps.message.content === nextProps.message.content &&
    prevProps.message.artifacts?.length === nextProps.message.artifacts?.length &&
    prevProps.isLast === nextProps.isLast
  );
});
