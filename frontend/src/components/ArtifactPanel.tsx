// src/components/ArtifactPanel.tsx
// REFACTORED: Editable path, keyboard shortcuts, single Save button, Apply button
// UPDATED: Added toast notifications for save/apply feedback

import React, { useState, useCallback, useEffect } from 'react';
import { X, Copy, Save, FileText, Code, CheckCircle, Check, AlertCircle } from 'lucide-react';
import { useArtifacts } from '../hooks/useArtifacts';
import { MonacoEditor } from './MonacoEditor';

interface Toast {
  id: string;
  message: string;
  type: 'success' | 'error' | 'info';
}

export const ArtifactPanel: React.FC = () => {
  const { 
    artifacts, 
    activeArtifact, 
    setActiveArtifact, 
    closeArtifacts,
    save,
    apply,
    copyArtifact,
    updateArtifact,
    updatePath,
    removeArtifact
  } = useArtifacts();
  
  const [isEditingPath, setIsEditingPath] = useState(false);
  const [pathInput, setPathInput] = useState('');
  const [toasts, setToasts] = useState<Toast[]>([]);
  
  // Update path input when active artifact changes
  useEffect(() => {
    if (activeArtifact) {
      setPathInput(activeArtifact.path);
    }
  }, [activeArtifact?.id]);
  
  // Toast helper
  const showToast = useCallback((message: string, type: 'success' | 'error' | 'info') => {
    const id = `toast-${Date.now()}-${Math.random()}`;
    const toast: Toast = { id, message, type };
    
    setToasts(prev => [...prev, toast]);
    
    // Auto-dismiss after 3 seconds
    setTimeout(() => {
      setToasts(prev => prev.filter(t => t.id !== id));
    }, 3000);
  }, []);
  
  const handleContentChange = useCallback((newContent: string | undefined) => {
    if (activeArtifact && newContent !== undefined) {
      updateArtifact(activeArtifact.id, { 
        content: newContent,
        status: 'draft'  // Mark as draft when editing
      });
    }
  }, [activeArtifact, updateArtifact]);

  const handleSave = useCallback(async () => {
    if (!activeArtifact) return;
    
    try {
      await save(activeArtifact.id);
      showToast(`Saved ${activeArtifact.path}`, 'success');
    } catch (error) {
      console.error('Save failed:', error);
      showToast(`Failed to save ${activeArtifact.path}`, 'error');
    }
  }, [activeArtifact, save, showToast]);

  const handleApply = useCallback(async () => {
    if (!activeArtifact) return;
    
    try {
      await apply(activeArtifact.id);
      showToast(`Applied ${activeArtifact.path} to workspace`, 'success');
    } catch (error) {
      console.error('Apply failed:', error);
      showToast(`Failed to apply ${activeArtifact.path}`, 'error');
    }
  }, [activeArtifact, apply, showToast]);

  const handlePathEdit = useCallback(() => {
    if (activeArtifact) {
      setPathInput(activeArtifact.path);
      setIsEditingPath(true);
    }
  }, [activeArtifact]);

  const handlePathSave = useCallback(() => {
    if (activeArtifact && pathInput.trim()) {
      updatePath(activeArtifact.id, pathInput.trim());
    }
    setIsEditingPath(false);
  }, [activeArtifact, pathInput, updatePath]);

  const handlePathKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      handlePathSave();
    } else if (e.key === 'Escape') {
      setIsEditingPath(false);
      if (activeArtifact) {
        setPathInput(activeArtifact.path);
      }
    }
  }, [handlePathSave, activeArtifact]);

  const handleCopy = useCallback(() => {
    if (!activeArtifact) return;
    copyArtifact(activeArtifact.id);
    showToast('Copied to clipboard', 'info');
  }, [activeArtifact, copyArtifact, showToast]);

  // Global keyboard shortcuts
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Cmd/Ctrl + S = Save
      if ((e.metaKey || e.ctrlKey) && e.key === 's') {
        e.preventDefault();
        handleSave();
      }
      // Cmd/Ctrl + Enter = Apply
      if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
        e.preventDefault();
        handleApply();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [handleSave, handleApply]);

  const getDisplayName = (path: string): string => {
    return path.split('/').pop() || path;
  };

  const getIcon = (language?: string) => {
    if (language && ['javascript', 'typescript', 'rust', 'python', 'json', 'css', 'html', 'markdown'].includes(language)) {
      return <Code size={16} />;
    }
    return <FileText size={16} />;
  };

  const getStatusBadge = (status?: string) => {
    if (!status || status === 'draft') return null;
    
    return (
      <span className={`text-xs px-1.5 py-0.5 rounded ${
        status === 'applied' 
          ? 'bg-green-900/30 text-green-400'
          : 'bg-blue-900/30 text-blue-400'
      }`}>
        {status === 'applied' ? 'Applied' : 'Saved'}
      </span>
    );
  };

  if (artifacts.length === 0) {
    return (
      <div className="h-full w-full border-l border-gray-700 bg-gray-900 flex flex-col">
        <div className="flex-1 flex items-center justify-center p-8 text-center">
          <div className="text-gray-400">
            <FileText size={48} className="mx-auto mb-4 opacity-50" />
            <h3 className="text-lg font-medium mb-2">No Artifacts Yet</h3>
            <p className="text-sm">
              Ask Mira to create code or documents and they'll appear here for editing.
            </p>
          </div>
        </div>
      </div>
    );
  }

  if (!activeArtifact) return null;

  return (
    <div className="h-full w-full border-l border-gray-700 bg-gray-900 flex flex-col relative">
      {/* Toast notifications */}
      <div className="absolute top-4 right-4 z-50 space-y-2">
        {toasts.map((toast) => (
          <div
            key={toast.id}
            className={`flex items-center gap-2 px-4 py-3 rounded-lg shadow-lg border backdrop-blur-sm animate-in slide-in-from-right duration-300 ${
              toast.type === 'success'
                ? 'bg-green-900/90 border-green-500/50 text-green-100'
                : toast.type === 'error'
                ? 'bg-red-900/90 border-red-500/50 text-red-100'
                : 'bg-blue-900/90 border-blue-500/50 text-blue-100'
            }`}
          >
            {toast.type === 'success' && <Check size={18} />}
            {toast.type === 'error' && <AlertCircle size={18} />}
            <span className="text-sm font-medium">{toast.message}</span>
          </div>
        ))}
      </div>

      {/* Header with tabs */}
      <div className="flex-shrink-0 border-b border-gray-700">
        <div className="flex items-center overflow-x-auto">
          {artifacts.map((artifact) => (
            <div
              key={artifact.id}
              className={`flex items-center gap-2 px-3 py-2 text-sm border-b-2 whitespace-nowrap transition-colors ${
                artifact.id === activeArtifact.id
                  ? 'border-blue-500 text-blue-400 bg-blue-900/20'
                  : 'border-transparent text-gray-400 hover:text-gray-200 hover:bg-gray-800'
              }`}
            >
              <button
                onClick={() => setActiveArtifact(artifact.id)}
                className="flex items-center gap-2"
              >
                {getIcon(artifact.language)}
                <span className="max-w-[120px] truncate">
                  {getDisplayName(artifact.path)}
                </span>
                {getStatusBadge(artifact.status)}
              </button>
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  removeArtifact(artifact.id);
                }}
                className="ml-1 p-0.5 hover:text-red-400 hover:bg-gray-700 rounded transition-colors"
                title="Close"
              >
                <X size={14} />
              </button>
            </div>
          ))}
        </div>
        
        {/* Path editor + action bar */}
        <div className="px-3 py-2 border-b border-gray-800">
          <div className="flex items-center justify-between gap-3">
            {/* Editable path */}
            <div className="flex-1 min-w-0">
              {isEditingPath ? (
                <input
                  type="text"
                  value={pathInput}
                  onChange={(e) => setPathInput(e.target.value)}
                  onBlur={handlePathSave}
                  onKeyDown={handlePathKeyDown}
                  className="w-full px-2 py-1 text-sm bg-gray-800 border border-blue-500 rounded text-gray-200 focus:outline-none focus:ring-1 focus:ring-blue-500"
                  autoFocus
                />
              ) : (
                <button
                  onClick={handlePathEdit}
                  className="text-sm text-gray-400 hover:text-gray-200 truncate max-w-full text-left"
                  title="Click to edit path"
                >
                  {activeArtifact.path}
                </button>
              )}
            </div>
            
            {/* Actions */}
            <div className="flex items-center gap-2 flex-shrink-0">
              <button
                onClick={handleCopy}
                className="p-1.5 text-gray-400 hover:text-gray-200 hover:bg-gray-800 rounded transition-colors"
                title="Copy to clipboard"
              >
                <Copy size={16} />
              </button>
              <button
                onClick={handleSave}
                className="flex items-center gap-1.5 px-2.5 py-1.5 text-sm text-blue-400 hover:text-white hover:bg-blue-600/20 border border-blue-500/30 rounded transition-colors"
                title="Save (Cmd/Ctrl+S)"
              >
                <Save size={14} />
                Save
              </button>
              <button
                onClick={handleApply}
                className="flex items-center gap-1.5 px-2.5 py-1.5 text-sm text-green-400 hover:text-white hover:bg-green-600/20 border border-green-500/30 rounded transition-colors"
                title="Apply to workspace (Cmd/Ctrl+Enter)"
              >
                <CheckCircle size={14} />
                Apply
              </button>
              <button
                onClick={closeArtifacts}
                className="p-1.5 text-gray-400 hover:text-gray-200 hover:bg-gray-800 rounded transition-colors"
                title="Close panel"
              >
                <X size={16} />
              </button>
            </div>
          </div>
        </div>
      </div>
      
      {/* Editor - takes remaining height */}
      <div className="flex-1 min-h-0 overflow-hidden">
        <MonacoEditor
          value={activeArtifact.content}
          language={activeArtifact.language || 'plaintext'}
          onChange={handleContentChange}
          options={{ readOnly: false }}
        />
      </div>
    </div>
  );
};
