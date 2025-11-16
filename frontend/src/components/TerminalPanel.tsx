// src/components/TerminalPanel.tsx
// Terminal panel container that manages multiple terminal sessions

import React, { useState } from 'react';
import { useTerminalStore } from '../stores/useTerminalStore';
import { useAppState } from '../stores/useAppState';
import { Terminal } from './Terminal';
import { Plus, ChevronRight, ChevronLeft } from 'lucide-react';

export const TerminalPanel: React.FC = () => {
  const {
    sessions,
    activeSessionId,
    isTerminalVisible,
    terminalWidth,
    setActiveSession,
    hideTerminal,
    setTerminalWidth,
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

  const sessionList = Object.values(sessions);
  const activeSession = activeSessionId ? sessions[activeSessionId] : null;
  const showNewTerminal = sessionList.length === 0 && currentProject && isTerminalVisible;

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
          <ChevronRight size={14} />
        </button>
        </div>

        {/* Terminal content */}
        <div className="flex-1 overflow-hidden relative">
          {/* Render all terminal sessions (keep them mounted) */}
          {sessionList.map((session) => (
            <div
              key={session.id}
              className={`absolute inset-0 ${
                session.id === activeSessionId ? '' : 'hidden'
              }`}
            >
              <Terminal
                sessionId={session.id}
                projectId={session.projectId}
                workingDirectory={session.workingDirectory}
                onClose={() => setActiveSession(null)}
              />
            </div>
          ))}

          {/* New terminal when no sessions exist */}
          {showNewTerminal && (
            <Terminal
              projectId={currentProject.id}
              workingDirectory={undefined}
              onClose={hideTerminal}
            />
          )}

          {/* Placeholder when no project */}
          {sessionList.length === 0 && !currentProject && (
            <div className="flex items-center justify-center h-full text-slate-400">
              <p>Select a project to open a terminal</p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
};

export default TerminalPanel;
