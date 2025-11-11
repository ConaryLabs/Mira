// src/components/CodebaseAttachModal.tsx
import { useState } from 'react';
import { X, Folder, Github } from 'lucide-react';

interface CodebaseAttachModalProps {
  projectId: string;
  onClose: () => void;
  onAttach: (type: 'local' | 'git', data: { path?: string; url?: string }) => Promise<void>;
}

export function CodebaseAttachModal({ projectId, onClose, onAttach }: CodebaseAttachModalProps) {
  const [activeTab, setActiveTab] = useState<'local' | 'git'>('local');
  const [attaching, setAttaching] = useState(false);
  const [statusMessage, setStatusMessage] = useState('');
  
  // Local state
  const [localPath, setLocalPath] = useState('');
  
  // Git state
  const [gitUrl, setGitUrl] = useState('');

  const handleSubmit = async () => {
    if (attaching) return;
    
    setAttaching(true);
    setStatusMessage('');
    
    try {
      if (activeTab === 'local') {
        if (!localPath.trim()) return;
        setStatusMessage('Attaching local directory...');
        await onAttach('local', { path: localPath.trim() });
      } else {
        if (!gitUrl.trim()) return;
        setStatusMessage('Importing repository (this may take a minute)...');
        await onAttach('git', { 
          url: gitUrl.trim()
        });
      }
      setStatusMessage('Success! Refreshing project list...');
      setTimeout(() => {
        onClose();
      }, 500);
    } catch (error) {
      console.error('Attach failed:', error);
      setStatusMessage(`Error: ${error instanceof Error ? error.message : 'Attach failed'}`);
      setAttaching(false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSubmit();
    }
    if (e.key === 'Escape') {
      onClose();
    }
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-slate-800 rounded-lg border border-slate-700 p-6 w-full max-w-lg">
        {/* Header */}
        <div className="flex items-center justify-between mb-6">
          <h2 className="text-xl font-bold text-slate-100">Attach Codebase</h2>
          <button
            onClick={onClose}
            className="p-1 text-slate-400 hover:text-slate-100 hover:bg-slate-700 rounded transition-colors"
          >
            <X size={20} />
          </button>
        </div>

        {/* Tabs */}
        <div className="flex gap-2 mb-6 border-b border-slate-700">
          <button
            onClick={() => setActiveTab('local')}
            className={`flex items-center gap-2 px-4 py-2 font-medium transition-colors border-b-2 -mb-px ${
              activeTab === 'local'
                ? 'border-blue-500 text-blue-400'
                : 'border-transparent text-slate-400 hover:text-slate-300'
            }`}
          >
            <Folder size={16} />
            Local Directory
          </button>
          <button
            onClick={() => setActiveTab('git')}
            className={`flex items-center gap-2 px-4 py-2 font-medium transition-colors border-b-2 -mb-px ${
              activeTab === 'git'
                ? 'border-blue-500 text-blue-400'
                : 'border-transparent text-slate-400 hover:text-slate-300'
            }`}
          >
            <Github size={16} />
            Git Repository
          </button>
        </div>

        {/* Tab Content */}
        <div className="space-y-4">
          {activeTab === 'local' ? (
            <>
              <p className="text-slate-400 text-sm mb-4">
                Link this project to a local directory on your machine for file operations.
              </p>
              <div>
                <label className="block text-sm font-medium text-slate-300 mb-2">
                  Directory Path *
                </label>
                <input
                  type="text"
                  value={localPath}
                  onChange={(e) => setLocalPath(e.target.value)}
                  onKeyDown={handleKeyDown}
                  placeholder="/home/user/my-project"
                  className="w-full px-3 py-2 bg-slate-900 border border-slate-600 rounded-lg text-slate-100 placeholder-slate-500 focus:outline-none focus:border-blue-500"
                  autoFocus
                />
                <p className="text-xs text-slate-500 mt-2">
                  Absolute path to the project directory on your local machine
                </p>
              </div>
            </>
          ) : (
            <>
              <p className="text-slate-400 text-sm mb-4">
                Import a Git repository to enable version control and file operations.
              </p>
              <div>
                <label className="block text-sm font-medium text-slate-300 mb-2">
                  Repository URL *
                </label>
                <input
                  type="text"
                  value={gitUrl}
                  onChange={(e) => setGitUrl(e.target.value)}
                  onKeyDown={handleKeyDown}
                  placeholder="https://github.com/username/repo.git"
                  className="w-full px-3 py-2 bg-slate-900 border border-slate-600 rounded-lg text-slate-100 placeholder-slate-500 focus:outline-none focus:border-blue-500"
                  autoFocus
                />
                <p className="text-xs text-slate-500 mt-2">
                  HTTPS or SSH URL of the repository
                </p>
              </div>
            </>
          )}
        </div>

        {/* Status Message */}
        {statusMessage && (
          <div className={`mt-4 p-3 rounded-lg text-sm ${
            statusMessage.startsWith('Error') 
              ? 'bg-red-500/10 text-red-400 border border-red-500/20'
              : statusMessage.startsWith('Success')
              ? 'bg-green-500/10 text-green-400 border border-green-500/20'
              : 'bg-blue-500/10 text-blue-400 border border-blue-500/20'
          }`}>
            {statusMessage}
          </div>
        )}

        {/* Actions */}
        <div className="flex gap-3 mt-6">
          <button
            onClick={handleSubmit}
            disabled={
              attaching || 
              (activeTab === 'local' && !localPath.trim()) ||
              (activeTab === 'git' && !gitUrl.trim())
            }
            className="flex-1 px-4 py-2 bg-blue-600 hover:bg-blue-700 disabled:bg-slate-700 disabled:text-slate-500 rounded-lg text-white font-medium transition-colors"
          >
            {attaching 
              ? 'Attaching...' 
              : activeTab === 'local' 
                ? 'Attach Directory' 
                : 'Import Repository'
            }
          </button>
          <button
            onClick={onClose}
            className="px-4 py-2 bg-slate-700 hover:bg-slate-600 rounded-lg text-slate-300 font-medium transition-colors"
          >
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}
