// src/components/Terminal.tsx
// Traditional terminal with xterm.js

import React, { useEffect, useRef, useState } from 'react';
import { Terminal as XTerm } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import { useBackendCommands } from '../services/BackendCommands';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import '@xterm/xterm/css/xterm.css';

interface TerminalProps {
  projectId: string;
  workingDirectory?: string;
}

export const Terminal: React.FC<TerminalProps> = ({
  projectId,
  workingDirectory,
}) => {
  const terminalRef = useRef<HTMLDivElement>(null);
  const xtermRef = useRef<XTerm | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const backendCommands = useBackendCommands();
  const subscribe = useWebSocketStore(state => state.subscribe);

  // Initialize xterm
  useEffect(() => {
    if (!terminalRef.current) return;

    // Create terminal instance
    const term = new XTerm({
      cursorBlink: true,
      fontSize: 14,
      fontFamily: 'Menlo, Monaco, "Courier New", monospace',
      theme: {
        background: '#0f172a', // slate-900
        foreground: '#e2e8f0', // slate-200
        cursor: '#60a5fa', // blue-400
        black: '#1e293b',
        red: '#ef4444',
        green: '#10b981',
        yellow: '#f59e0b',
        blue: '#3b82f6',
        magenta: '#a855f7',
        cyan: '#06b6d4',
        white: '#cbd5e1',
        brightBlack: '#475569',
        brightRed: '#f87171',
        brightGreen: '#34d399',
        brightYellow: '#fbbf24',
        brightBlue: '#60a5fa',
        brightMagenta: '#c084fc',
        brightCyan: '#22d3ee',
        brightWhite: '#f1f5f9',
      },
      allowProposedApi: true,
    });

    // Create fit addon
    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);

    // Open terminal in DOM
    term.open(terminalRef.current);
    fitAddon.fit();

    // Store refs
    xtermRef.current = term;
    fitAddonRef.current = fitAddon;

    // Handle resize
    const handleResize = () => {
      fitAddon.fit();
      if (sessionId) {
        backendCommands.resizeTerminal(sessionId, term.cols, term.rows);
      }
    };

    window.addEventListener('resize', handleResize);

    // Cleanup
    return () => {
      window.removeEventListener('resize', handleResize);
      term.dispose();
    };
  }, []);

  // Start backend terminal session
  useEffect(() => {
    if (!xtermRef.current || sessionId) return;

    const term = xtermRef.current;

    const startSession = async () => {
      try {
        const cols = term.cols;
        const rows = term.rows;

        term.write('Connecting to terminal...\r\n');
        await backendCommands.startTerminal(projectId, workingDirectory, cols, rows);
      } catch (err) {
        console.error('[Terminal] Failed to start session:', err);
        term.write('\x1b[31mFailed to start terminal session\x1b[0m\r\n');
      }
    };

    startSession();
  }, [projectId, workingDirectory, sessionId, backendCommands]);

  // Handle terminal input
  useEffect(() => {
    if (!xtermRef.current) return;

    const term = xtermRef.current;

    const handleData = (data: string) => {
      if (sessionId) {
        // Send input to backend (base64 encoded)
        const base64Data = btoa(data);
        backendCommands.sendTerminalInput(sessionId, base64Data);
      }
    };

    const disposable = term.onData(handleData);

    return () => {
      disposable.dispose();
    };
  }, [sessionId, backendCommands]);

  // Subscribe to WebSocket messages
  useEffect(() => {
    if (!xtermRef.current) return;

    const term = xtermRef.current;

    const unsubscribe = subscribe('terminal-xterm', (message) => {
      // Handle terminal session started
      if (message.type === 'data' && message.data) {
        const data = message.data;

        // Terminal session started
        if (data.session_id && data.project_id && data.working_directory) {
          if (data.project_id === projectId && !sessionId) {
            setSessionId(data.session_id);
            term.clear();
            console.log('[Terminal] Session started:', data.session_id);
          }
          return;
        }
      }

      // Handle terminal output
      if (message.type === 'terminal_output' && message.session_id && message.data) {
        if (message.session_id === sessionId) {
          try {
            // Decode base64 data
            const decoded = atob(message.data);
            term.write(decoded);
          } catch (e) {
            console.error('[Terminal] Failed to decode output:', e);
          }
        }
        return;
      }

      // Handle terminal closed
      if (message.type === 'terminal_closed' && message.session_id === sessionId) {
        term.write('\r\n\x1b[33mTerminal session closed\x1b[0m\r\n');
        setSessionId(null);
      }

      // Handle terminal error
      if (message.type === 'terminal_error' && message.session_id === sessionId) {
        term.write(`\r\n\x1b[31mError: ${message.error}\x1b[0m\r\n`);
      }
    });

    return () => {
      unsubscribe();
    };
  }, [subscribe, sessionId, projectId]);

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      if (sessionId) {
        try {
          backendCommands.closeTerminal(sessionId);
        } catch (err) {
          console.error('[Terminal] Error closing terminal:', err);
        }
      }
    };
  }, [sessionId, backendCommands]);

  return (
    <div className="flex flex-col h-full bg-slate-900">
      <div ref={terminalRef} className="flex-1 p-2" />
    </div>
  );
};

export default Terminal;
