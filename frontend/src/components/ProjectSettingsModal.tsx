// frontend/src/components/ProjectSettingsModal.tsx
// Modal for project settings including guidelines editor

import React, { useState, useEffect, useCallback } from 'react';
import { X, Save, FileText, Loader2 } from 'lucide-react';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useAppState } from '../stores/useAppState';

interface ProjectSettingsModalProps {
  projectId: string;
  projectName: string;
  isOpen: boolean;
  onClose: () => void;
}

export const ProjectSettingsModal: React.FC<ProjectSettingsModalProps> = ({
  projectId,
  projectName,
  isOpen,
  onClose,
}) => {
  const { send, subscribe } = useWebSocketStore();
  const { addToast } = useAppState();

  const [guidelines, setGuidelines] = useState('');
  const [originalGuidelines, setOriginalGuidelines] = useState('');
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);

  // Load guidelines when modal opens
  useEffect(() => {
    if (!isOpen || !projectId) return;

    setLoading(true);

    // Subscribe to response
    const unsubscribe = subscribe('guidelines-load', (message) => {
      if (message.type === 'data' && message.data?.type === 'guidelines') {
        if (message.data.project_id === projectId) {
          const content = message.data.content || '';
          setGuidelines(content);
          setOriginalGuidelines(content);
          setLoading(false);
        }
      }
    });

    // Request guidelines
    send({
      type: 'project_command',
      method: 'guidelines.get',
      params: { project_id: projectId }
    });

    return unsubscribe;
  }, [isOpen, projectId, send, subscribe]);

  // Save guidelines
  const handleSave = useCallback(async () => {
    if (guidelines === originalGuidelines) {
      addToast({ message: 'No changes to save', type: 'info' });
      return;
    }

    setSaving(true);

    try {
      // Subscribe to response
      const unsubscribe = subscribe('guidelines-save', (message) => {
        if (message.type === 'data' && message.data?.type === 'guidelines_updated') {
          if (message.data.project_id === projectId) {
            setOriginalGuidelines(guidelines);
            addToast({ message: 'Guidelines saved', type: 'success' });
            setSaving(false);
            unsubscribe();
          }
        } else if (message.type === 'error') {
          addToast({ message: 'Failed to save guidelines', type: 'error' });
          setSaving(false);
          unsubscribe();
        }
      });

      // Send save request
      send({
        type: 'project_command',
        method: 'guidelines.set',
        params: {
          project_id: projectId,
          content: guidelines
        }
      });

      // Timeout after 5 seconds
      setTimeout(() => {
        if (saving) {
          setSaving(false);
          addToast({ message: 'Save request timed out', type: 'error' });
        }
      }, 5000);
    } catch (error) {
      console.error('Failed to save guidelines:', error);
      addToast({ message: 'Failed to save guidelines', type: 'error' });
      setSaving(false);
    }
  }, [guidelines, originalGuidelines, projectId, send, subscribe, addToast, saving]);

  const handleClose = useCallback(() => {
    if (guidelines !== originalGuidelines) {
      if (!confirm('You have unsaved changes. Discard them?')) {
        return;
      }
    }
    setGuidelines('');
    setOriginalGuidelines('');
    onClose();
  }, [guidelines, originalGuidelines, onClose]);

  const hasChanges = guidelines !== originalGuidelines;

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-slate-900 border border-slate-700 rounded-lg shadow-2xl w-full max-w-3xl max-h-[80vh] flex flex-col">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-slate-700 flex-shrink-0">
          <div className="flex items-center gap-2">
            <FileText size={18} className="text-blue-400" />
            <h3 className="text-lg font-semibold text-slate-200">
              Project Settings: {projectName}
            </h3>
          </div>
          <button
            onClick={handleClose}
            className="p-1.5 text-slate-400 hover:text-slate-200 hover:bg-slate-700 rounded transition-colors"
            disabled={saving}
          >
            <X size={18} />
          </button>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-hidden flex flex-col p-4">
          <div className="mb-3">
            <label className="block text-sm font-medium text-slate-300 mb-1">
              Project Guidelines
            </label>
            <p className="text-xs text-slate-500 mb-2">
              Guidelines are automatically included in the AI context when working with this project.
              Use markdown format for best results.
            </p>
          </div>

          {loading ? (
            <div className="flex-1 flex items-center justify-center">
              <Loader2 size={24} className="text-slate-400 animate-spin" />
            </div>
          ) : (
            <textarea
              value={guidelines}
              onChange={(e) => setGuidelines(e.target.value)}
              placeholder={`# ${projectName} Guidelines

## Project Overview
Describe what this project does...

## Code Style
- Use consistent formatting
- Follow project conventions

## Important Notes
Add any special instructions for the AI...`}
              className="flex-1 w-full px-3 py-2 bg-slate-800 border border-slate-600 rounded text-slate-200 placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500 resize-none font-mono text-sm"
              disabled={saving}
            />
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between px-4 py-3 border-t border-slate-700 flex-shrink-0">
          <div className="text-xs text-slate-500">
            {hasChanges && <span className="text-amber-400">Unsaved changes</span>}
          </div>
          <div className="flex gap-2">
            <button
              type="button"
              onClick={handleClose}
              className="px-4 py-2 text-sm bg-slate-700 hover:bg-slate-600 text-slate-200 rounded transition-colors"
              disabled={saving}
            >
              Cancel
            </button>
            <button
              type="button"
              onClick={handleSave}
              className="flex items-center gap-2 px-4 py-2 text-sm bg-blue-600 hover:bg-blue-700 disabled:bg-slate-700 disabled:cursor-not-allowed text-white rounded transition-colors"
              disabled={saving || loading || !hasChanges}
            >
              {saving ? (
                <>
                  <Loader2 size={16} className="animate-spin" />
                  Saving...
                </>
              ) : (
                <>
                  <Save size={16} />
                  Save Guidelines
                </>
              )}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
