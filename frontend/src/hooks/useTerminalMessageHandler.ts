// src/hooks/useTerminalMessageHandler.ts
// Hook to handle terminal WebSocket messages

import { useEffect, useRef } from 'react';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useTerminalStore } from '../stores/useTerminalStore';

// Terminal message handlers registry
// Maps session_id to xterm instance for writing output
const terminalInstances = new Map<string, any>();

export function registerTerminalInstance(sessionId: string, xtermInstance: any) {
  terminalInstances.set(sessionId, xtermInstance);
}

export function unregisterTerminalInstance(sessionId: string) {
  terminalInstances.delete(sessionId);
}

export function useTerminalMessageHandler() {
  const { addSession, updateSession, removeSession } = useTerminalStore();
  const unsubscribeRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    const subscribe = useWebSocketStore.getState().subscribe;

    const unsubscribe = subscribe('terminal-handler', (message) => {
      // Handle data envelope
      if (message.type === 'data' && message.data) {
        const data = message.data;

        // Terminal session started
        if (data.session_id && data.project_id && data.working_directory) {
          addSession({
            id: data.session_id,
            projectId: data.project_id,
            workingDirectory: data.working_directory,
            isActive: true,
            createdAt: new Date().toISOString(),
            cols: data.cols || 80,
            rows: data.rows || 24,
          });
          return;
        }

        // Terminal sessions list
        if (data.sessions && Array.isArray(data.sessions)) {
          // Handle terminal sessions list
          // This could be used to restore sessions on reconnect
          return;
        }
      }

      // Terminal output (if sent as dedicated message type)
      if (message.type === 'terminal_output' && message.session_id && message.data) {
        const xterm = terminalInstances.get(message.session_id);
        if (xterm) {
          // Decode base64 data
          try {
            const decoded = atob(message.data);
            xterm.write(decoded);
          } catch (e) {
            console.error('[Terminal] Failed to decode output:', e);
          }
        }
      }

      // Terminal closed
      if (message.type === 'terminal_closed' && message.session_id) {
        updateSession(message.session_id, { isActive: false });
      }

      // Terminal error
      if (message.type === 'terminal_error' && message.session_id && message.error) {
        const xterm = terminalInstances.get(message.session_id);
        if (xterm) {
          xterm.writeln(`\r\n\x1b[1;31mError: ${message.error}\x1b[0m`);
        }
      }
    });

    unsubscribeRef.current = unsubscribe;

    return () => {
      if (unsubscribeRef.current) {
        unsubscribeRef.current();
      }
    };
  }, [addSession, updateSession, removeSession]);
}
