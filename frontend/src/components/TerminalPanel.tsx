// src/components/TerminalPanel.tsx
// Terminal panel container that manages multiple terminal sessions

import React, { useState } from 'react';
import { useTerminalStore } from '../stores/useTerminalStore';
import { useAppState } from '../stores/useAppState';
import Terminal from './Terminal';
import { Plus, ChevronDown, ChevronUp } from 'lucide-react';

export const TerminalPanel: React.FC = () => {
  const {
    sessions,
    activeSessionId,
    isTerminalVisible,
    terminalHeight,
    setActiveSession,
    hideTerminal,
    setTerminalHeight,
  } = useTerminalStore();

  const { currentProject } = useAppState();
  const [isResizing, setIsResizing] = useState(false);
  const [resizeStartY, setResizeStartY] = useState(0);
  const [resizeStartHeight, setResizeStartHeight] = useState(0);

  if (!isTerminalVisible) {
    return null;
  }

  const handleResizeStart = (e: React.MouseEvent) => {
    setIsResizing(true);
    setResizeStartY(e.clientY);
    setResizeStartHeight(terminalHeight);
    e.preventDefault();
  };

  React.useEffect(() => {
    if (!isResizing) return;

    const handleMouseMove = (e: MouseEvent) => {
      const deltaY = resizeStartY - e.clientY;
      const viewportHeight = window.innerHeight;
      const deltaPercent = (deltaY / viewportHeight) * 100;
      const newHeight = resizeStartHeight + deltaPercent;
      setTerminalHeight(newHeight);
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
  }, [isResizing, resizeStartY, resizeStartHeight, setTerminalHeight]);

  const sessionList = Object.values(sessions);
  const activeSession = activeSessionId ? sessions[activeSessionId] : null;

  return (
    <div
      className="fixed bottom-0 left-0 right-0 flex flex-col bg-slate-900 border-t border-slate-700 shadow-2xl z-40"
      style={{ height: `${terminalHeight}vh` }}
    >
      {/* Resize handle */}
      <div
        className={`h-1 cursor-ns-resize hover:bg-blue-500 transition-colors ${
          isResizing ? 'bg-blue-500' : 'bg-slate-700'
        }`}
        onMouseDown={handleResizeStart}
      />

      {/* Terminal tabs */}
      <div className="flex items-center gap-2 px-3 py-2 bg-slate-800 border-b border-slate-700 overflow-x-auto">
        {sessionList.map((session) => (
          <button
            key={session.id}
            onClick={() => setActiveSession(session.id)}
            className={`px-3 py-1.5 text-xs font-mono rounded transition-colors whitespace-nowrap ${
              session.id === activeSessionId
                ? 'bg-slate-700 text-slate-100'
                : 'bg-slate-900 text-slate-400 hover:bg-slate-700 hover:text-slate-200'
            }`}
          >
            {session.workingDirectory.split('/').pop() || 'Terminal'}
            <span className="ml-2 text-slate-500">
              ({session.id.substring(0, 6)})
            </span>
          </button>
        ))}

        <button
          onClick={() => {
            // TODO: Open new terminal for current project
            if (currentProject) {
              // This will be handled by the Terminal component
            }
          }}
          className="p-1.5 text-slate-400 hover:text-slate-200 hover:bg-slate-700 rounded transition-colors"
          title="New terminal"
        >
          <Plus size={14} />
        </button>

        <div className="flex-1" />

        <button
          onClick={hideTerminal}
          className="p-1.5 text-slate-400 hover:text-slate-200 hover:bg-slate-700 rounded transition-colors"
          title="Hide terminal"
        >
          <ChevronDown size={14} />
        </button>
      </div>

      {/* Terminal content */}
      <div className="flex-1 overflow-hidden">
        {activeSession && (
          <Terminal
            sessionId={activeSession.id}
            projectId={activeSession.projectId}
            workingDirectory={activeSession.workingDirectory}
            onClose={() => setActiveSession(null)}
          />
        )}
        {!activeSession && sessionList.length === 0 && currentProject && (
          <Terminal
            projectId={currentProject.id}
            workingDirectory={undefined}
            onClose={hideTerminal}
          />
        )}
      </div>
    </div>
  );
};

export default TerminalPanel;
