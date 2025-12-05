// src/components/OpenDirectoryModal.tsx
// Modal for opening a local directory as a project

import React, { useState } from 'react';
import { X, FolderOpen } from 'lucide-react';

interface OpenDirectoryModalProps {
  isOpen: boolean;
  onClose: () => void;
  onOpen: (path: string) => Promise<boolean>;
  opening: boolean;
}

export const OpenDirectoryModal: React.FC<OpenDirectoryModalProps> = ({
  isOpen,
  onClose,
  onOpen,
  opening,
}) => {
  const [path, setPath] = useState('');

  if (!isOpen) return null;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    const success = await onOpen(path);
    if (success) {
      setPath('');
      onClose();
    }
  };

  const handleClose = () => {
    setPath('');
    onClose();
  };

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-white dark:bg-slate-900 border border-gray-200 dark:border-slate-700 rounded-lg shadow-2xl w-full max-w-md">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-gray-200 dark:border-slate-700">
          <h3 className="text-lg font-semibold text-gray-800 dark:text-slate-200">Open Directory</h3>
          <button
            onClick={handleClose}
            className="p-1.5 text-gray-500 dark:text-slate-400 hover:text-gray-700 dark:hover:text-slate-200 hover:bg-gray-100 dark:hover:bg-slate-700 rounded transition-colors"
            disabled={opening}
          >
            <X size={18} />
          </button>
        </div>

        {/* Form */}
        <form onSubmit={handleSubmit} className="p-4 space-y-4">
          <div>
            <label htmlFor="directory-path" className="block text-sm font-medium text-gray-700 dark:text-slate-300 mb-1">
              Directory Path
            </label>
            <input
              id="directory-path"
              type="text"
              value={path}
              onChange={(e) => setPath(e.target.value)}
              placeholder="/home/user/my-project"
              className="w-full px-3 py-2 bg-gray-50 dark:bg-slate-800 border border-gray-300 dark:border-slate-600 rounded text-gray-900 dark:text-slate-200 placeholder-gray-500 dark:placeholder-slate-500 focus:outline-none focus:ring-2 focus:ring-blue-500"
              disabled={opening}
              autoFocus
              required
            />
            <p className="mt-1 text-xs text-gray-500 dark:text-slate-500">
              Enter the full path to your project directory
            </p>
          </div>

          {/* Actions */}
          <div className="flex gap-2 justify-end pt-2">
            <button
              type="button"
              onClick={handleClose}
              className="px-4 py-2 text-sm bg-gray-200 dark:bg-slate-700 hover:bg-gray-300 dark:hover:bg-slate-600 text-gray-700 dark:text-slate-200 rounded transition-colors"
              disabled={opening}
            >
              Cancel
            </button>
            <button
              type="submit"
              className="flex items-center gap-2 px-4 py-2 text-sm bg-blue-600 hover:bg-blue-700 disabled:bg-blue-400 dark:disabled:bg-blue-800 disabled:cursor-not-allowed text-white rounded transition-colors"
              disabled={opening || !path.trim()}
            >
              <FolderOpen size={16} />
              {opening ? 'Opening...' : 'Open Directory'}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
};
