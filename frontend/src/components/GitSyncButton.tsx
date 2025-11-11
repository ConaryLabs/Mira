// src/components/GitSyncButton.tsx

import React, { useState } from 'react';
import { Download, Upload, RefreshCw, Check, AlertCircle } from 'lucide-react';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useAppState } from '../stores/useAppState';

export const GitSyncButton: React.FC = () => {
  const [syncing, setSyncing] = useState(false);
  const [status, setStatus] = useState<'idle' | 'success' | 'error'>('idle');
  const { send } = useWebSocketStore();
  const { currentProject } = useAppState();
  
  const handlePull = async () => {
    if (!currentProject || syncing) return;
    
    setSyncing(true);
    setStatus('idle');
    
    try {
      await send({
        type: 'git_command',
        method: 'git.pull',
        params: {
          project_id: currentProject.id
        }
      });
      
      setStatus('success');
      setTimeout(() => setStatus('idle'), 3000);
      
      // Optionally refresh the file tree after pull
      await send({
        type: 'git_command',
        method: 'git.tree',
        params: {
          project_id: currentProject.id,
          recursive: true
        }
      });
      
    } catch (error) {
      console.error('Failed to pull from remote:', error);
      setStatus('error');
      setTimeout(() => setStatus('idle'), 3000);
    } finally {
      setSyncing(false);
    }
  };
  
  const handlePush = async () => {
    if (!currentProject || syncing) return;
    
    setSyncing(true);
    setStatus('idle');
    
    try {
      // First commit any changes
      await send({
        type: 'git_command',
        method: 'git.sync',
        params: {
          project_id: currentProject.id,
          message: `Update from Mira: ${new Date().toLocaleString()}`
        }
      });
      
      setStatus('success');
      setTimeout(() => setStatus('idle'), 3000);
      
    } catch (error) {
      console.error('Failed to push to remote:', error);
      setStatus('error');
      setTimeout(() => setStatus('idle'), 3000);
    } finally {
      setSyncing(false);
    }
  };
  
  if (!currentProject?.has_repository) {
    return null;
  }
  
  return (
    <div className="flex items-center gap-2">
      {/* Pull button */}
      <button
        onClick={handlePull}
        disabled={syncing}
        className={`p-2 rounded-md transition-colors ${
          syncing 
            ? 'text-gray-500 cursor-not-allowed' 
            : status === 'success'
            ? 'text-green-500'
            : status === 'error'
            ? 'text-red-500'
            : 'text-gray-400 hover:text-gray-200 hover:bg-gray-800'
        }`}
        title="Pull from GitHub"
      >
        {syncing && status === 'idle' ? (
          <RefreshCw className="w-4 h-4 animate-spin" />
        ) : status === 'success' ? (
          <Check className="w-4 h-4" />
        ) : status === 'error' ? (
          <AlertCircle className="w-4 h-4" />
        ) : (
          <Download className="w-4 h-4" />
        )}
      </button>
      
      {/* Push button */}
      <button
        onClick={handlePush}
        disabled={syncing}
        className={`p-2 rounded-md transition-colors ${
          syncing 
            ? 'text-gray-500 cursor-not-allowed' 
            : 'text-gray-400 hover:text-gray-200 hover:bg-gray-800'
        }`}
        title="Push to GitHub"
      >
        <Upload className="w-4 h-4" />
      </button>
    </div>
  );
};
