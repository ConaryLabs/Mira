// src/components/CommandOutputViewer.tsx
// Read-only command output viewer (replaces interactive xterm terminal)
// Shows streaming command output with collapsible blocks

import React, { useEffect, useRef } from 'react';
import { ChevronDown, ChevronRight, Terminal, CheckCircle, XCircle, Loader } from 'lucide-react';
import { useTerminalStore, CommandBlock } from '../stores/useTerminalStore';

interface CommandOutputViewerProps {
  sessionId: string;
}

const CommandBlockItem: React.FC<{ block: CommandBlock; sessionId: string }> = ({ block, sessionId }) => {
  const { toggleCommandCollapse } = useTerminalStore();
  const outputRef = useRef<HTMLDivElement>(null);

  // Auto-scroll to bottom when output updates and block is running
  useEffect(() => {
    if (block.isRunning && outputRef.current && !block.isCollapsed) {
      outputRef.current.scrollTop = outputRef.current.scrollHeight;
    }
  }, [block.output, block.isRunning, block.isCollapsed]);

  const hasOutput = block.output.trim().length > 0;

  return (
    <div className="border border-gray-700 rounded-lg overflow-hidden bg-gray-800/30">
      {/* Command Header */}
      <div
        className="flex items-center gap-2 p-3 bg-gray-800/50 cursor-pointer hover:bg-gray-700/50 transition-colors"
        onClick={() => toggleCommandCollapse(sessionId, block.id)}
      >
        {/* Collapse Icon */}
        {block.isCollapsed ? (
          <ChevronRight className="w-4 h-4 text-gray-400 flex-shrink-0" />
        ) : (
          <ChevronDown className="w-4 h-4 text-gray-400 flex-shrink-0" />
        )}

        {/* Status Icon */}
        {block.isRunning ? (
          <Loader className="w-4 h-4 text-blue-400 animate-spin flex-shrink-0" />
        ) : block.exitCode === 0 ? (
          <CheckCircle className="w-4 h-4 text-green-400 flex-shrink-0" />
        ) : (
          <XCircle className="w-4 h-4 text-red-400 flex-shrink-0" />
        )}

        {/* Command Text */}
        <code className="flex-1 text-sm font-mono text-slate-100 truncate">
          $ {block.command}
        </code>

        {/* Exit Code Badge */}
        {!block.isRunning && block.exitCode !== null && (
          <span className={`text-xs px-2 py-0.5 rounded ${
            block.exitCode === 0
              ? 'bg-green-900/30 text-green-400'
              : 'bg-red-900/30 text-red-400'
          }`}>
            exit {block.exitCode}
          </span>
        )}

        {/* Timestamp */}
        <span className="text-xs text-gray-500">
          {new Date(block.timestamp).toLocaleTimeString()}
        </span>
      </div>

      {/* Command Output */}
      {!block.isCollapsed && hasOutput && (
        <div
          ref={outputRef}
          className="p-4 bg-black/40 font-mono text-sm text-gray-300 overflow-y-auto max-h-96 whitespace-pre-wrap break-words"
        >
          {block.output}
        </div>
      )}

      {/* No Output Message */}
      {!block.isCollapsed && !hasOutput && !block.isRunning && (
        <div className="p-4 bg-black/40 text-sm text-gray-500 italic">
          No output
        </div>
      )}
    </div>
  );
};

export const CommandOutputViewer: React.FC<CommandOutputViewerProps> = ({ sessionId }) => {
  const { sessions } = useTerminalStore();
  const session = sessions[sessionId];
  const containerRef = useRef<HTMLDivElement>(null);

  // Auto-scroll to bottom when new command blocks are added
  useEffect(() => {
    if (containerRef.current) {
      const { scrollHeight, clientHeight, scrollTop } = containerRef.current;
      const isNearBottom = scrollHeight - (scrollTop + clientHeight) < 100;
      if (isNearBottom) {
        containerRef.current.scrollTop = scrollHeight;
      }
    }
  }, [session?.commandBlocks.length]);

  if (!session) {
    return (
      <div className="h-full flex items-center justify-center text-gray-500">
        <div className="text-center">
          <Terminal className="w-12 h-12 mx-auto mb-4 opacity-50" />
          <p>No active session</p>
        </div>
      </div>
    );
  }

  const commandBlocks = session.commandBlocks || [];

  return (
    <div
      ref={containerRef}
      className="h-full overflow-y-auto p-4 space-y-3 bg-gray-900"
    >
      {commandBlocks.length === 0 ? (
        <div className="h-full flex items-center justify-center text-gray-500">
          <div className="text-center">
            <Terminal className="w-12 h-12 mx-auto mb-4 opacity-50" />
            <p>No commands executed yet</p>
            <p className="text-sm mt-2">Command output will appear here</p>
          </div>
        </div>
      ) : (
        commandBlocks.map((block) => (
          <CommandBlockItem
            key={block.id}
            block={block}
            sessionId={sessionId}
          />
        ))
      )}
    </div>
  );
};
