// src/components/MessageList.tsx
// FIXED: Display streaming content as a virtual message that updates in real-time

import React, { useEffect, useRef, useState, useCallback, useMemo } from 'react';
import { Virtuoso, VirtuosoHandle } from 'react-virtuoso';
import { ArrowDown } from 'lucide-react';
import { useChatStore, ChatMessage as StoreChatMessage } from '../stores/useChatStore';
import { ChatMessage } from './ChatMessage';
import { ThinkingIndicator } from './ThinkingIndicator';

const EmptyState: React.FC = () => (
  <div className="flex items-center justify-center h-full text-gray-500 dark:text-gray-400">
    <div className="text-center">
      <p className="text-lg mb-2">No messages yet</p>
      <p className="text-sm text-gray-400 dark:text-gray-500">Start a conversation to see messages here</p>
    </div>
  </div>
);

const Footer: React.FC<{ isWaiting: boolean }> = ({ isWaiting }) => {
  if (!isWaiting) return null;
  
  return (
    <div className="px-4 py-2">
      <ThinkingIndicator />
    </div>
  );
};

export const MessageList: React.FC = () => {
  const messages = useChatStore(state => state.messages);
  const isStreaming = useChatStore(state => state.isStreaming);
  const streamingContent = useChatStore(state => state.streamingContent);
  const streamingMessageId = useChatStore(state => state.streamingMessageId);
  const isWaitingForResponse = useChatStore(state => state.isWaitingForResponse);
  
  // DEBUG: Log streaming state changes
  useEffect(() => {
    if (isStreaming) {
      console.log('[MessageList] Streaming active, content length:', streamingContent.length);
    }
  }, [isStreaming, streamingContent]);
  
  const virtuosoRef = useRef<VirtuosoHandle>(null);
  const [showScrollButton, setShowScrollButton] = useState(false);
  const [atBottom, setAtBottom] = useState(true);
  const lastMessageCountRef = useRef(0);

  // CRITICAL FIX: Create virtual streaming message that updates as chunks arrive
  const displayMessages = useMemo(() => {
    if (isStreaming && streamingContent && streamingMessageId) {
      // DEBUG: Log to verify re-renders
      console.log('[MessageList] Creating virtual streaming message:', streamingContent.length, 'chars');
      
      // Add a virtual message with the streaming content
      return [
        ...messages,
        {
          id: streamingMessageId,
          role: 'assistant' as const,
          content: streamingContent,
          timestamp: Date.now(),
          isStreaming: true  // Flag to show streaming cursor
        }
      ];
    }
    return messages;
  }, [messages, isStreaming, streamingContent, streamingMessageId]);

  const handleAtBottomStateChange = useCallback((bottom: boolean) => {
    setAtBottom(bottom);
    setShowScrollButton(!bottom);
  }, []);

  const scrollToBottom = useCallback(() => {
    virtuosoRef.current?.scrollToIndex({
      index: 'LAST',
      behavior: 'smooth',
      align: 'end'
    });
  }, []);

  // Auto-scroll when message count changes
  useEffect(() => {
    const messageCountChanged = displayMessages.length !== lastMessageCountRef.current;
    lastMessageCountRef.current = displayMessages.length;

    if (messageCountChanged) {
      setTimeout(() => {
        scrollToBottom();
      }, 50);
    }
  }, [displayMessages.length, scrollToBottom]);

  // Auto-scroll when streaming content updates (keeps text visible as it arrives)
  useEffect(() => {
    if (isStreaming && streamingContent) {
      setTimeout(() => {
        scrollToBottom();
      }, 10); // Very short delay for smooth streaming
    }
  }, [streamingContent, isStreaming, scrollToBottom]);

  // Auto-scroll when waiting state changes to true (thinking indicator appears)
  useEffect(() => {
    if (isWaitingForResponse) {
      setTimeout(() => {
        scrollToBottom();
      }, 100);
    }
  }, [isWaitingForResponse, scrollToBottom]);

  // Initial scroll to bottom on mount
  useEffect(() => {
    if (displayMessages.length > 0 && virtuosoRef.current) {
      setTimeout(() => {
        virtuosoRef.current?.scrollToIndex({
          index: 'LAST',
          behavior: 'auto',
          align: 'end'
        });
      }, 200);
    }
  }, []); // Only run on mount

  if (displayMessages.length === 0 && !isWaitingForResponse) {
    return <EmptyState />;
  }

  return (
    <div className="h-full relative chat-scroll">
      <Virtuoso
        ref={virtuosoRef}
        data={displayMessages}
        overscan={200}
        itemContent={(index, message: StoreChatMessage & { isStreaming?: boolean }) => (
          <div className="px-4 py-2">
            <ChatMessage message={message} />
          </div>
        )}
        followOutput={false}
        initialTopMostItemIndex={displayMessages.length > 0 ? displayMessages.length - 1 : 0}
        atBottomStateChange={handleAtBottomStateChange}
        atBottomThreshold={50}
        alignToBottom
        components={{
          Footer: () => <Footer isWaiting={isWaitingForResponse && !isStreaming} />
        }}
      />
      
      {/* Scroll to bottom button */}
      {showScrollButton && (
        <button
          onClick={scrollToBottom}
          className="absolute bottom-4 right-4 p-2 bg-blue-600 hover:bg-blue-700 text-white rounded-full shadow-lg transition-all"
          aria-label="Scroll to bottom"
        >
          <ArrowDown className="w-5 h-5" />
        </button>
      )}
    </div>
  );
};

export default MessageList;
