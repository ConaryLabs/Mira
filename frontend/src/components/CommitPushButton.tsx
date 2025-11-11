// src/components/CommitPushButton.tsx
import React, { useState } from 'react';
import { GitCommit } from 'lucide-react';
import { useAppState } from '../stores/useAppState';
import { useBackendCommands } from '../services/BackendCommands';

export const CommitPushButton: React.FC = () => {
  const [showCommitDialog, setShowCommitDialog] = useState(false);
  const [commitMessage, setCommitMessage] = useState('');
  const [isCommitting, setIsCommitting] = useState(false);
  
  const { currentProject, modifiedFiles, clearModifiedFiles } = useAppState();
  const commands = useBackendCommands();

  const handleCommit = async () => {
    if (!currentProject || modifiedFiles.length === 0) return;
    
    setIsCommitting(true);
    
    try {
      // 🚀 The backend call that was missing!
      await commands.gitSync(
        currentProject.id,
        commitMessage || `Update ${modifiedFiles.length} files`
      );
      
      // Clear modified files and close dialog
      clearModifiedFiles();
      setCommitMessage('');
      setShowCommitDialog(false);
      
      console.log('✅ Successfully committed and pushed changes');
    } catch (error) {
      console.error('❌ Commit failed:', error);
      // TODO: Show error toast to user
    } finally {
      setIsCommitting(false);
    }
  };

  const isDisabled = !currentProject || modifiedFiles.length === 0 || isCommitting;

  return (
    <>
      <button
        onClick={() => setShowCommitDialog(true)}
        disabled={isDisabled}
        className={`flex items-center gap-2 px-3 py-1.5 rounded-md font-medium text-sm transition-colors ${
          modifiedFiles.length > 0 && currentProject
            ? 'bg-green-600 text-white hover:bg-green-700'
            : 'bg-gray-200 dark:bg-gray-700 text-gray-500 dark:text-gray-400 cursor-not-allowed'
        }`}
        title={
          !currentProject 
            ? 'No project selected'
            : modifiedFiles.length === 0 
            ? 'No changes to commit'
            : `Commit ${modifiedFiles.length} file(s)`
        }
      >
        <GitCommit size={16} />
        <span>
          {isCommitting 
            ? 'Committing...' 
            : modifiedFiles.length > 0 
            ? `Commit (${modifiedFiles.length})`
            : 'Commit'
          }
        </span>
      </button>

      {/* Commit dialog */}
      {showCommitDialog && (
        <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50">
          <div className="bg-white dark:bg-gray-800 p-6 rounded-lg shadow-xl max-w-md w-full mx-4">
            <h3 className="text-lg font-semibold mb-4 text-gray-900 dark:text-gray-100">
              Commit Changes
            </h3>
            
            <div className="mb-4">
              <p className="text-sm text-gray-600 dark:text-gray-400 mb-2">
                {modifiedFiles.length} file(s) will be committed:
              </p>
              <div className="max-h-32 overflow-y-auto bg-gray-50 dark:bg-gray-700 p-2 rounded text-xs">
                {modifiedFiles.map((file) => (
                  <div key={file} className="text-gray-700 dark:text-gray-300">
                    {file}
                  </div>
                ))}
              </div>
            </div>
            
            <textarea
              value={commitMessage}
              onChange={(e) => setCommitMessage(e.target.value)}
              placeholder="Commit message (optional)"
              className="w-full p-3 border border-gray-200 dark:border-gray-600 rounded-md bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100 focus:outline-none focus:ring-2 focus:ring-blue-500"
              rows={3}
            />
            
            <div className="flex gap-3 mt-4">
              <button
                onClick={handleCommit}
                disabled={isCommitting}
                className="flex-1 bg-green-600 text-white py-2 px-4 rounded-md hover:bg-green-700 disabled:bg-gray-400"
              >
                {isCommitting ? 'Committing...' : 'Commit & Push'}
              </button>
              <button
                onClick={() => setShowCommitDialog(false)}
                disabled={isCommitting}
                className="px-4 py-2 text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-200"
              >
                Cancel
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
};
