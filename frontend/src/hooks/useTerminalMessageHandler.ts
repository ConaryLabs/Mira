// src/hooks/useTerminalMessageHandler.ts
// Hook to handle terminal WebSocket messages and route to command blocks

import { useEffect } from 'react';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useTerminalStore } from '../stores/useTerminalStore';

export function useTerminalMessageHandler() {
  useEffect(() => {
    const subscribe = useWebSocketStore.getState().subscribe;

    const unsubscribe = subscribe('terminal-handler', (message) => {
      // Get fresh state from store to avoid stale closures
      const store = useTerminalStore.getState();
      const {
        addSession,
        updateSession,
        appendCommandOutput,
        completeCommand,
        sessions,
      } = store;

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
        const sessionId = message.session_id;
        const session = sessions[sessionId];

        if (!session) {
          // Session not found - might arrive before session is registered
          // Log once per session to debug without spam
          if (!window.__missingTerminalSessions) {
            window.__missingTerminalSessions = new Set();
          }
          if (!window.__missingTerminalSessions.has(sessionId)) {
            console.debug('[Terminal] Output received for unregistered session:', sessionId);
            window.__missingTerminalSessions.add(sessionId);
          }
          return;
        }

        // Find the most recent running command block
        const runningBlock = session.commandBlocks
          .slice()
          .reverse()
          .find(block => block.isRunning);

        if (runningBlock) {
          try {
            // Decode base64 data
            const decoded = atob(message.data);

            // Append output to the running command block
            appendCommandOutput(sessionId, runningBlock.id, decoded);
          } catch (e) {
            console.error('[Terminal] Failed to decode output:', e);
          }
        }
        // No running block is normal - shell echoes, prompts, etc.
      }

      // Terminal command completed (if backend sends this)
      if (message.type === 'terminal_command_complete' && message.session_id && message.block_id) {
        const exitCode = message.exit_code ?? 0;
        completeCommand(message.session_id, message.block_id, exitCode);
      }

      // Terminal closed
      if (message.type === 'terminal_closed' && message.session_id) {
        updateSession(message.session_id, { isActive: false });
      }

      // Terminal error
      if (message.type === 'terminal_error' && message.session_id && message.error) {
        const session = sessions[message.session_id];
        if (session) {
          // Find the most recent running command block
          const runningBlock = session.commandBlocks
            .slice()
            .reverse()
            .find(block => block.isRunning);

          if (runningBlock) {
            appendCommandOutput(message.session_id, runningBlock.id, `\nError: ${message.error}\n`);
            completeCommand(message.session_id, runningBlock.id, 1);
          }
        }
      }
    });

    return () => {
      unsubscribe();
    };
  }, []); // Empty deps - we get fresh state inside the callback
}
