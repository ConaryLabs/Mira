// src/components/ChatArea.tsx
// Main chat layout container - zero props, all store-based

import React from 'react';
import { ConnectionBanner } from './ConnectionBanner';
import { MessageList } from './MessageList';
import { ChatInput } from './ChatInput';
import { SudoApprovalInline } from './SudoApprovalInline';
import { usePendingApprovals } from '../stores/useSudoStore';

export const ChatArea: React.FC = () => {
  const pendingApprovals = usePendingApprovals();

  return (
    <div className="flex-1 flex flex-col min-h-0">
      <ConnectionBanner />
      <div className="flex-1 overflow-hidden min-h-0">
        <MessageList />
      </div>
      {/* Sudo approval requests - shown above input for visibility */}
      {pendingApprovals.length > 0 && (
        <div className="flex-shrink-0 border-t border-yellow-500/30 bg-yellow-500/5 px-4 py-2 max-h-64 overflow-y-auto">
          {pendingApprovals.map((request) => (
            <SudoApprovalInline key={request.id} request={request} />
          ))}
        </div>
      )}
      <div className="flex-shrink-0 border-t border-gray-200 dark:border-slate-700 p-4">
        <ChatInput />
      </div>
    </div>
  );
};
