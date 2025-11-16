// src/components/TerminalPanel.tsx
// Terminal panel container with resize handle

import React, { useState, useEffect } from 'react';
import { useTerminalStore } from '../stores/useTerminalStore';
import { useAppState } from '../stores/useAppState';
import { CommandOutputViewer } from './CommandOutputViewer';
import { ChevronRight } from 'lucide-react';

export const TerminalPanel: React.FC = () => {
  const {
    isTerminalVisible,
    terminalWidth,
    hideTerminal,
    setTerminalWidth,
    activeSessionId,
  } = useTerminalStore();

  const { currentProject } = useAppState();
  const [isResizing, setIsResizing] = useState(false);
  const [resizeStartX, setResizeStartX] = useState(0);
  const [resizeStartWidth, setResizeStartWidth] = useState(0);

  // Define handlers before early return (for hooks rule compliance)
  const handleResizeStart = (e: React.MouseEvent) => {
    setIsResizing(true);
    setResizeStartX(e.clientX);
    setResizeStartWidth(terminalWidth);
    e.preventDefault();
  };

  // All hooks must be called before any conditional returns
  React.useEffect(() => {
    if (!isResizing) return;

    const handleMouseMove = (e: MouseEvent) => {
      const deltaX = resizeStartX - e.clientX;
      const viewportWidth = window.innerWidth;
      const deltaPercent = (deltaX / viewportWidth) * 100;
      const newWidth = resizeStartWidth + deltaPercent;
      setTerminalWidth(newWidth);
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
  }, [isResizing, resizeStartX, resizeStartWidth, setTerminalWidth]);

  return (
    <div
      className={`flex flex-row bg-slate-900 border-l border-slate-700 shadow-2xl transition-all duration-300 ${
        isTerminalVisible ? '' : 'w-0 border-l-0'
      }`}
      style={{
        width: isTerminalVisible ? `${terminalWidth}vw` : '0',
        overflow: isTerminalVisible ? 'visible' : 'hidden'
      }}
    >
      {/* Resize handle */}
      <div
        className={`w-1 cursor-ew-resize hover:bg-blue-500 transition-colors ${
          isResizing ? 'bg-blue-500' : 'bg-slate-700'
        }`}
        onMouseDown={handleResizeStart}
      />

      <div className={`flex-1 flex flex-col overflow-hidden ${
        isTerminalVisible ? '' : 'hidden'
      }`}>
        {/* Terminal header */}
        <div className="flex items-center gap-2 px-3 py-2 bg-slate-800 border-b border-slate-700 overflow-x-auto">
          <span className="text-sm text-slate-300 font-mono">Terminal</span>
          {currentProject && (
            <span className="text-xs text-slate-500">
              {currentProject.name}
            </span>
          )}

          <div className="flex-1" />

          <button
            onClick={hideTerminal}
            className="p-1.5 text-slate-400 hover:text-slate-200 hover:bg-slate-700 rounded transition-colors"
            title="Hide terminal"
          >
            <ChevronRight size={14} />
          </button>
        </div>

        {/* Terminal content */}
        <div className="flex-1 overflow-hidden relative">
          {activeSessionId ? (
            <CommandOutputViewer sessionId={activeSessionId} />
          ) : (
            <div className="flex items-center justify-center h-full text-slate-400">
              <p>No active terminal session</p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
};

export default TerminalPanel;
