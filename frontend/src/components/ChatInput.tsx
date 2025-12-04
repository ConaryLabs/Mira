// src/components/ChatInput.tsx
// Simplified - waiting state now handled in useChatMessaging
import React, { useRef, KeyboardEvent, useCallback, useEffect } from 'react';
import { Send } from 'lucide-react';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useAppState } from '../stores/useAppState';
import { useChatStore } from '../stores/useChatStore';
import { useUIStore, useInputContent } from '../stores/useUIStore';
import { useChatMessaging } from '../hooks/useChatMessaging';

export const ChatInput: React.FC = () => {
  const content = useInputContent();
  const setInputContent = useUIStore(state => state.setInputContent);
  const clearInput = useUIStore(state => state.clearInput);
  
  const connectionState = useWebSocketStore(state => state.connectionState);
  const currentProject = useAppState(state => state.currentProject);
  const isWaitingForResponse = useChatStore(state => state.isWaitingForResponse);
  
  const { handleSend: sendMessage } = useChatMessaging();
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const handleSend = useCallback(async () => {
    const trimmedContent = content.trim();
    if (!trimmedContent || isWaitingForResponse || connectionState !== 'connected') return;
    
    await sendMessage(trimmedContent);  // FIXED: Send trimmed content
    clearInput();
    
    // Reset textarea height
    if (textareaRef.current) {
      textareaRef.current.style.height = 'auto';
    }
  }, [content, isWaitingForResponse, connectionState, sendMessage, clearInput]);

  const handleKeyDown = useCallback((e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  }, [handleSend]);

  const handleChange = useCallback((e: React.ChangeEvent<HTMLTextAreaElement>) => {
    setInputContent(e.target.value);
    
    // Auto-resize with proper async handling to avoid scroll-linked warnings
    if (textareaRef.current) {
      const textarea = textareaRef.current;
      // Reset height to get accurate scrollHeight
      textarea.style.height = 'auto';
      // Use requestAnimationFrame to decouple from scroll events
      requestAnimationFrame(() => {
        const newHeight = Math.min(textarea.scrollHeight, 200);
        textarea.style.height = `${newHeight}px`;
      });
    }
  }, [setInputContent]);

  // Auto-focus when connected
  useEffect(() => {
    if (connectionState === 'connected' && textareaRef.current) {
      textareaRef.current.focus();
    }
  }, [connectionState]);

  const isDisabled = connectionState !== 'connected' || isWaitingForResponse;

  return (
    <div className="flex gap-2 items-end w-full">
      <div className="flex-1 relative">
        <textarea
          ref={textareaRef}
          value={content}
          onChange={handleChange}
          onKeyDown={handleKeyDown}
          placeholder={
            connectionState !== 'connected'
              ? "Connecting to Mira..."
              : currentProject
              ? "Message Mira... (Shift+Enter for new line)"
              : "Select a project to start chatting..."
          }
          disabled={isDisabled}
          className="w-full px-4 py-3 bg-gray-50 dark:bg-slate-800 border border-gray-300 dark:border-slate-700 rounded-lg text-gray-900 dark:text-slate-100 placeholder-gray-500 dark:placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500 resize-none"
          rows={1}
          style={{ minHeight: '48px', maxHeight: '200px' }}
        />
      </div>
      
      <button
        onClick={handleSend}
        disabled={isDisabled || !content.trim()}
        className="p-3 bg-blue-600 hover:bg-blue-700 disabled:bg-gray-300 dark:disabled:bg-slate-700 disabled:cursor-not-allowed rounded-lg transition-colors"
        title="Send message (Enter)"
      >
        <Send size={20} className="text-white" />
      </button>
    </div>
  );
};
