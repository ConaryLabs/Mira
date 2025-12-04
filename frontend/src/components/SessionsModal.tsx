// src/components/SessionsModal.tsx
// Modal for managing chat sessions

import React, { useState, useEffect } from 'react';
import { X, Plus, Clock, MessageSquare, Trash2, GitFork, Edit2, Check, FolderOpen } from 'lucide-react';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useChatStore } from '../stores/useChatStore';
import { useSessionOperations } from '../hooks/useSessionOperations';
import type { Session } from '../types';

interface SessionsModalProps {
  isOpen: boolean;
  onClose: () => void;
}

export const SessionsModal: React.FC<SessionsModalProps> = ({ isOpen, onClose }) => {
  const { subscribe } = useWebSocketStore();
  const { currentSessionId } = useChatStore();

  const {
    sessions,
    setSessions,
    setLoading,
    loading,
    creating,
    deleting,
    refreshSessions,
    createSession,
    deleteSession,
    switchSession,
    updateSession,
    forkSession,
    formatLastActive,
  } = useSessionOperations();

  // Local state for editing
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editName, setEditName] = useState('');

  // Load sessions on mount
  useEffect(() => {
    if (!isOpen) return;

    console.log('SessionsModal: Loading sessions');
    refreshSessions();

    const unsubscribe = subscribe('sessions-modal', (message) => {
      if (message.type === 'data' && message.data?.type === 'session_list') {
        console.log('SessionsModal: Received sessions:', message.data.sessions?.length || 0);
        setSessions(message.data.sessions || []);
        setLoading(false);
      } else if (message.type === 'data' && message.data?.type === 'session_created') {
        // A new session was created, refresh the list
        refreshSessions();
      }
    });

    return unsubscribe;
  }, [isOpen, subscribe, refreshSessions, setSessions, setLoading]);

  if (!isOpen) return null;

  const handleStartEdit = (session: Session) => {
    setEditingId(session.id);
    setEditName(session.name || '');
  };

  const handleSaveEdit = async () => {
    if (editingId && editName.trim()) {
      await updateSession(editingId, editName);
    }
    setEditingId(null);
    setEditName('');
  };

  const handleCancelEdit = () => {
    setEditingId(null);
    setEditName('');
  };

  const handleSwitchAndClose = (session: Session) => {
    switchSession(session);
    onClose();
  };

  const getSessionDisplayName = (session: Session): string => {
    if (session.name) return session.name;
    if (session.project_path) {
      const parts = session.project_path.split('/');
      return parts[parts.length - 1] || session.project_path;
    }
    return `Session ${session.id.slice(0, 8)}`;
  };

  const getPreviewText = (session: Session): string => {
    return session.last_message_preview || '(no messages)';
  };

  // Group sessions by project
  const sessionsByProject = sessions.reduce((acc, session) => {
    const key = session.project_path || 'No Project';
    if (!acc[key]) acc[key] = [];
    acc[key].push(session);
    return acc;
  }, {} as Record<string, Session[]>);

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-white dark:bg-slate-900 border border-gray-200 dark:border-slate-700 rounded-lg shadow-2xl w-full max-w-2xl max-h-[80vh] flex flex-col">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-gray-200 dark:border-slate-700">
          <div>
            <h2 className="text-lg font-semibold text-gray-900 dark:text-slate-200">Sessions</h2>
            <p className="text-sm text-gray-500 dark:text-slate-400">
              {sessions.length} session{sessions.length !== 1 ? 's' : ''}
            </p>
          </div>
          <div className="flex items-center gap-2">
            <button
              onClick={() => createSession()}
              disabled={creating}
              className="flex items-center gap-2 px-3 py-1.5 bg-blue-600 hover:bg-blue-700 disabled:bg-blue-400 text-white rounded transition-colors text-sm"
            >
              <Plus size={16} />
              {creating ? 'Creating...' : 'New Session'}
            </button>
            <button
              onClick={onClose}
              className="p-1.5 text-gray-500 dark:text-slate-400 hover:text-gray-700 dark:hover:text-slate-200 hover:bg-gray-100 dark:hover:bg-slate-700 rounded transition-colors"
              title="Close"
            >
              <X size={18} />
            </button>
          </div>
        </div>

        {/* Session List */}
        <div className="flex-1 overflow-y-auto p-4">
          {loading ? (
            <div className="flex items-center justify-center py-12">
              <div className="text-gray-500 dark:text-slate-400">Loading sessions...</div>
            </div>
          ) : sessions.length === 0 ? (
            <div className="flex flex-col items-center justify-center py-12 text-center">
              <MessageSquare size={48} className="text-gray-400 dark:text-slate-600 mb-4" />
              <h3 className="text-lg font-medium text-gray-500 dark:text-slate-400 mb-2">No Sessions Yet</h3>
              <p className="text-sm text-gray-400 dark:text-slate-500 mb-4">
                Create a new session to start chatting
              </p>
              <button
                onClick={() => createSession()}
                disabled={creating}
                className="flex items-center gap-2 px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded transition-colors"
              >
                <Plus size={18} />
                Create Session
              </button>
            </div>
          ) : (
            <div className="space-y-6">
              {Object.entries(sessionsByProject).map(([projectPath, projectSessions]) => (
                <div key={projectPath}>
                  {/* Project Group Header */}
                  <div className="flex items-center gap-2 mb-2 text-sm text-gray-500 dark:text-slate-400">
                    <FolderOpen size={14} />
                    <span>{projectPath}</span>
                  </div>

                  {/* Sessions in this project */}
                  <div className="space-y-2">
                    {projectSessions.map((session) => (
                      <div
                        key={session.id}
                        className={`
                          p-3 rounded-lg border-2 transition-all cursor-pointer
                          ${currentSessionId === session.id
                            ? 'border-blue-500 bg-blue-50 dark:bg-blue-900/20'
                            : 'border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-800 hover:border-gray-300 dark:hover:border-slate-600'
                          }
                        `}
                        onClick={() => handleSwitchAndClose(session)}
                      >
                        <div className="flex items-start justify-between gap-3">
                          {/* Session Info */}
                          <div className="flex-1 min-w-0">
                            {editingId === session.id ? (
                              <div className="flex items-center gap-2" onClick={(e) => e.stopPropagation()}>
                                <input
                                  type="text"
                                  value={editName}
                                  onChange={(e) => setEditName(e.target.value)}
                                  className="flex-1 px-2 py-1 text-sm border border-gray-300 dark:border-slate-600 rounded bg-white dark:bg-slate-700 text-gray-900 dark:text-slate-200"
                                  autoFocus
                                  onKeyDown={(e) => {
                                    if (e.key === 'Enter') handleSaveEdit();
                                    if (e.key === 'Escape') handleCancelEdit();
                                  }}
                                />
                                <button
                                  onClick={handleSaveEdit}
                                  className="p-1 text-green-600 hover:bg-green-100 dark:hover:bg-green-900/30 rounded"
                                >
                                  <Check size={16} />
                                </button>
                                <button
                                  onClick={handleCancelEdit}
                                  className="p-1 text-gray-500 hover:bg-gray-100 dark:hover:bg-slate-700 rounded"
                                >
                                  <X size={16} />
                                </button>
                              </div>
                            ) : (
                              <>
                                <h3 className="text-sm font-medium text-gray-800 dark:text-slate-200 truncate">
                                  {getSessionDisplayName(session)}
                                </h3>
                                <p className="text-xs text-gray-500 dark:text-slate-400 truncate mt-1">
                                  {getPreviewText(session)}
                                </p>
                              </>
                            )}

                            {/* Metadata */}
                            <div className="flex items-center gap-3 mt-2 text-xs text-gray-400 dark:text-slate-500">
                              <span className="flex items-center gap-1">
                                <MessageSquare size={12} />
                                {session.message_count} messages
                              </span>
                              <span className="flex items-center gap-1">
                                <Clock size={12} />
                                {formatLastActive(session.last_active)}
                              </span>
                            </div>
                          </div>

                          {/* Actions */}
                          <div className="flex items-center gap-1" onClick={(e) => e.stopPropagation()}>
                            <button
                              onClick={() => handleStartEdit(session)}
                              className="p-1.5 text-gray-400 hover:text-gray-600 dark:hover:text-slate-300 hover:bg-gray-100 dark:hover:bg-slate-700 rounded transition-colors"
                              title="Rename session"
                            >
                              <Edit2 size={14} />
                            </button>
                            <button
                              onClick={() => forkSession(session.id)}
                              disabled={creating}
                              className="p-1.5 text-gray-400 hover:text-blue-600 dark:hover:text-blue-400 hover:bg-blue-50 dark:hover:bg-blue-900/30 rounded transition-colors"
                              title="Fork session"
                            >
                              <GitFork size={14} />
                            </button>
                            <button
                              onClick={() => deleteSession(session.id)}
                              disabled={deleting === session.id}
                              className="p-1.5 text-gray-400 hover:text-red-600 dark:hover:text-red-400 hover:bg-red-50 dark:hover:bg-red-900/30 rounded transition-colors"
                              title="Delete session"
                            >
                              <Trash2 size={14} />
                            </button>
                          </div>
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
