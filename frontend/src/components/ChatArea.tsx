// src/components/ChatArea.tsx
// Main chat layout container - zero props, all store-based

import React from 'react';
import { ConnectionBanner } from './ConnectionBanner';
import { MessageList } from './MessageList';
import { ChatInput } from './ChatInput';

export const ChatArea: React.FC = () => {
  return (
    <div className="flex-1 flex flex-col min-h-0">
      <ConnectionBanner />
      <div className="flex-1 overflow-hidden min-h-0">
        <MessageList />
      </div>
      <div className="flex-shrink-0 border-t border-gray-200 dark:border-slate-700 p-4">
        <ChatInput />
      </div>
    </div>
  );
};
