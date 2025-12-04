// src/hooks/useSessionOperations.ts
// Custom hook for session CRUD operations

import { useCallback, useState } from 'react';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useChatStore } from '../stores/useChatStore';
import { useAppState } from '../stores/useAppState';
import type { Session } from '../types';

export const useSessionOperations = () => {
  const { send } = useWebSocketStore();
  const { setSessionId, clearMessages } = useChatStore();
  const { addToast, currentProject } = useAppState();

  const [sessions, setSessions] = useState<Session[]>([]);
  const [currentSession, setCurrentSession] = useState<Session | null>(null);
  const [loading, setLoading] = useState(false);
  const [creating, setCreating] = useState(false);
  const [deleting, setDeleting] = useState<string | null>(null);

  const refreshSessions = useCallback(() => {
    setLoading(true);
    send({
      type: 'session_command',
      method: 'session.list',
      params: {
        project_path: currentProject?.name || undefined,
        limit: 50
      }
    });
  }, [send, currentProject]);

  const createSession = useCallback(async (name?: string, projectPath?: string) => {
    setCreating(true);
    try {
      await send({
        type: 'session_command',
        method: 'session.create',
        params: {
          name: name?.trim() || undefined,
          project_path: projectPath || currentProject?.name || undefined
        }
      });

      addToast({ message: 'Created new session', type: 'success' });
      setTimeout(refreshSessions, 100);
      return true;
    } catch (error) {
      console.error('Create session failed:', error);
      addToast({ message: 'Failed to create session', type: 'error' });
      return false;
    } finally {
      setCreating(false);
    }
  }, [send, addToast, refreshSessions, currentProject]);

  const deleteSession = useCallback(async (sessionId: string) => {
    setDeleting(sessionId);

    try {
      await send({
        type: 'session_command',
        method: 'session.delete',
        params: { id: sessionId }
      });

      // If deleting current session, clear messages
      const { currentSessionId } = useChatStore.getState();
      if (currentSessionId === sessionId) {
        clearMessages();
        setCurrentSession(null);
      }

      addToast({ message: 'Session deleted', type: 'success' });
      setTimeout(refreshSessions, 100);
      return true;
    } catch (error) {
      console.error('Delete session failed:', error);
      addToast({ message: 'Failed to delete session', type: 'error' });
      return false;
    } finally {
      setDeleting(null);
    }
  }, [send, addToast, refreshSessions, clearMessages]);

  const switchSession = useCallback((session: Session) => {
    setSessionId(session.id);
    setCurrentSession(session);
    clearMessages(); // Clear messages when switching sessions
    addToast({ message: `Switched to session: ${session.name || session.id.slice(0, 8)}`, type: 'success' });
  }, [setSessionId, clearMessages, addToast]);

  const updateSession = useCallback(async (sessionId: string, name: string) => {
    try {
      await send({
        type: 'session_command',
        method: 'session.update',
        params: {
          id: sessionId,
          name: name.trim()
        }
      });

      addToast({ message: 'Session updated', type: 'success' });
      setTimeout(refreshSessions, 100);
      return true;
    } catch (error) {
      console.error('Update session failed:', error);
      addToast({ message: 'Failed to update session', type: 'error' });
      return false;
    }
  }, [send, addToast, refreshSessions]);

  const forkSession = useCallback(async (sourceId: string, name?: string) => {
    setCreating(true);
    try {
      await send({
        type: 'session_command',
        method: 'session.fork',
        params: {
          source_id: sourceId,
          name: name?.trim() || undefined
        }
      });

      addToast({ message: 'Session forked', type: 'success' });
      setTimeout(refreshSessions, 100);
      return true;
    } catch (error) {
      console.error('Fork session failed:', error);
      addToast({ message: 'Failed to fork session', type: 'error' });
      return false;
    } finally {
      setCreating(false);
    }
  }, [send, addToast, refreshSessions]);

  // Format relative time for display
  const formatLastActive = useCallback((timestamp: number): string => {
    const now = Date.now() / 1000; // Convert to seconds
    const diff = now - timestamp;

    if (diff < 60) return 'just now';
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    if (diff < 604800) return `${Math.floor(diff / 86400)}d ago`;

    return new Date(timestamp * 1000).toLocaleDateString('en-US', {
      month: 'short',
      day: 'numeric',
      year: 'numeric'
    });
  }, []);

  return {
    sessions,
    setSessions,
    currentSession,
    setCurrentSession,
    loading,
    setLoading,
    creating,
    deleting,
    refreshSessions,
    createSession,
    deleteSession,
    switchSession,
    updateSession,
    forkSession,
    formatLastActive,
  };
};
