// src/components/Terminal.tsx
// Terminal component using xterm.js with themed styling

import React, { useEffect, useRef, useState } from 'react';
import { Terminal as XTerm } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';
import { useTerminalStore } from '../stores/useTerminalStore';
import { useBackendCommands } from '../services/BackendCommands';
import { registerTerminalInstance, unregisterTerminalInstance } from '../hooks/useTerminalMessageHandler';
import { X, Maximize2, Minimize2 } from 'lucide-react';

interface TerminalProps {
  sessionId?: string;
  projectId: string;
  workingDirectory?: string;
  onClose?: () => void;
}

export const Terminal: React.FC<TerminalProps> = ({
  sessionId: initialSessionId,
  projectId,
  workingDirectory,
  onClose,
}) => {
  const terminalRef = useRef<HTMLDivElement>(null);
  const xtermRef = useRef<XTerm | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const [sessionId, setSessionId] = useState<string | null>(initialSessionId || null);
  const [isMaximized, setIsMaximized] = useState(false);
  const backendCommands = useBackendCommands();

  const { updateSession, removeSession } = useTerminalStore();

  useEffect(() => {
    if (!terminalRef.current) return;

    // Create xterm instance with themed styling
    const xterm = new XTerm({
      cursorBlink: true,
      fontSize: 14,
      fontFamily: '"Cascadia Code", "Fira Code", "Consolas", "Courier New", monospace',
      theme: {
        background: '#0f172a',      // slate-900
        foreground: '#f1f5f9',      // slate-100
        cursor: '#3b82f6',          // blue-500
        cursorAccent: '#0f172a',    // slate-900
        selectionBackground: '#334155',  // slate-700
        black: '#1e293b',           // slate-800
        red: '#ef4444',             // red-500
        green: '#22c55e',           // green-500
        yellow: '#eab308',          // yellow-500
        blue: '#3b82f6',            // blue-500
        magenta: '#a855f7',         // purple-500
        cyan: '#06b6d4',            // cyan-500
        white: '#cbd5e1',           // slate-300
        brightBlack: '#475569',     // slate-600
        brightRed: '#f87171',       // red-400
        brightGreen: '#4ade80',     // green-400
        brightYellow: '#facc15',    // yellow-400
        brightBlue: '#60a5fa',      // blue-400
        brightMagenta: '#c084fc',   // purple-400
        brightCyan: '#22d3ee',      // cyan-400
        brightWhite: '#f1f5f9',     // slate-100
      },
      scrollback: 10000,
      convertEol: true,
    });

    const fitAddon = new FitAddon();
    xterm.loadAddon(fitAddon);

    xterm.open(terminalRef.current);
    fitAddon.fit();

    xtermRef.current = xterm;
    fitAddonRef.current = fitAddon;

    // Register terminal instance for message handling
    if (sessionId) {
      registerTerminalInstance(sessionId, xterm);
    }

    // Handle terminal input (user typing)
    xterm.onData((data) => {
      if (sessionId) {
        // Encode data to base64 for transmission
        const base64Data = btoa(data);
        backendCommands.sendTerminalInput(sessionId, base64Data);
      }
    });

    // Start terminal session if not already started
    if (!sessionId) {
      const dims = fitAddon.proposeDimensions();
      backendCommands
        .startTerminal(
          projectId,
          workingDirectory,
          dims?.cols || 80,
          dims?.rows || 24
        )
        .then(() => {
          xterm.writeln('\x1b[1;32mTerminal session starting...\x1b[0m');
        })
        .catch((err) => {
          xterm.writeln(`\x1b[1;31mFailed to start terminal: ${err}\x1b[0m`);
        });
    }

    // Handle resize
    const handleResize = () => {
      fitAddon.fit();
      if (sessionId) {
        const dims = fitAddon.proposeDimensions();
        if (dims) {
          backendCommands.resizeTerminal(sessionId, dims.cols, dims.rows);
          updateSession(sessionId, { cols: dims.cols, rows: dims.rows });
        }
      }
    };

    window.addEventListener('resize', handleResize);

    // Cleanup
    return () => {
      window.removeEventListener('resize', handleResize);
      if (sessionId) {
        unregisterTerminalInstance(sessionId);
        backendCommands.closeTerminal(sessionId);
      }
      xterm.dispose();
    };
  }, [projectId, workingDirectory]);

  // Update session ID when terminal starts
  useEffect(() => {
    // Listen for session start response
    // This will be set by the WebSocket message handler
  }, []);

  // Handle WebSocket messages for terminal output
  useEffect(() => {
    // This will be connected to WebSocket store
    // For now, placeholder for terminal output handling
    // The WebSocket store should route terminal output messages here
  }, [sessionId]);

  const handleClose = () => {
    if (sessionId) {
      backendCommands.closeTerminal(sessionId);
      removeSession(sessionId);
    }
    onClose?.();
  };

  const handleMaximize = () => {
    setIsMaximized(!isMaximized);
    // Trigger resize after maximizing
    setTimeout(() => {
      fitAddonRef.current?.fit();
    }, 100);
  };

  return (
    <div
      className={`flex flex-col bg-slate-900 border border-slate-700 rounded-lg shadow-lg ${
        isMaximized ? 'fixed inset-4 z-50' : 'h-full'
      }`}
    >
      {/* Terminal header */}
      <div className="flex items-center justify-between px-3 py-2 bg-slate-800 border-b border-slate-700 rounded-t-lg">
        <div className="flex items-center gap-2 text-sm text-slate-300">
          <span className="font-mono">Terminal</span>
          {sessionId && (
            <span className="text-xs text-slate-500">({sessionId.substring(0, 8)})</span>
          )}
          {workingDirectory && (
            <span className="text-xs text-slate-400">@ {workingDirectory}</span>
          )}
        </div>

        <div className="flex items-center gap-1">
          <button
            onClick={handleMaximize}
            className="p-1.5 text-slate-400 hover:text-slate-200 hover:bg-slate-700 rounded transition-colors"
            title={isMaximized ? 'Restore' : 'Maximize'}
          >
            {isMaximized ? <Minimize2 size={16} /> : <Maximize2 size={16} />}
          </button>
          <button
            onClick={handleClose}
            className="p-1.5 text-slate-400 hover:text-red-400 hover:bg-slate-700 rounded transition-colors"
            title="Close terminal"
          >
            <X size={16} />
          </button>
        </div>
      </div>

      {/* Terminal body */}
      <div ref={terminalRef} className="flex-1 p-2 overflow-hidden" />
    </div>
  );
};

export default Terminal;
