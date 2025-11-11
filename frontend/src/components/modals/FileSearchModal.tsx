// src/components/modals/FileSearchModal.tsx
// PHASE 2: File Search Modal for tool invocation
// Allows users to search through project files with filters

import React, { useState, useRef, useEffect } from 'react';
import { X, Search, File, Filter, Calendar } from 'lucide-react';
import clsx from 'clsx';

interface FileSearchModalProps {
  onSubmit: (query: string, filters?: FileSearchFilters) => void;
  onClose: () => void;
  isDark: boolean;
}

interface FileSearchFilters {
  file_extensions?: string[];
  date_range?: {
    start?: string;
    end?: string;
  };
  max_results?: number;
  include_content?: boolean;
}

export const FileSearchModal: React.FC<FileSearchModalProps> = ({
  onSubmit,
  onClose,
  isDark
}) => {
  const [query, setQuery] = useState('');
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [filters, setFilters] = useState<FileSearchFilters>({
    file_extensions: [],
    max_results: 20,
    include_content: true
  });
  const [extensionInput, setExtensionInput] = useState('');
  
  const modalRef = useRef<HTMLDivElement>(null);
  const queryInputRef = useRef<HTMLInputElement>(null);

  // Focus query input when modal opens
  useEffect(() => {
    queryInputRef.current?.focus();
  }, []);

  // Handle outside click to close
  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (modalRef.current && !modalRef.current.contains(event.target as Node)) {
        onClose();
      }
    };

    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, [onClose]);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!query.trim()) return;

    const searchFilters = {
      ...filters,
      file_extensions: filters.file_extensions?.length ? filters.file_extensions : undefined,
    };

    onSubmit(query.trim(), searchFilters);
  };

  const handleKeyPress = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape') {
      onClose();
    }
  };

  const addExtension = () => {
    if (extensionInput.trim() && !filters.file_extensions?.includes(extensionInput.trim())) {
      setFilters(prev => ({
        ...prev,
        file_extensions: [...(prev.file_extensions || []), extensionInput.trim()]
      }));
      setExtensionInput('');
    }
  };

  const removeExtension = (ext: string) => {
    setFilters(prev => ({
      ...prev,
      file_extensions: prev.file_extensions?.filter(e => e !== ext)
    }));
  };

  const commonExtensions = ['ts', 'tsx', 'js', 'jsx', 'py', 'rs', 'go', 'java', 'cpp', 'c', 'md', 'txt', 'json', 'yml', 'yaml'];

  return (
    <div className="fixed inset-0 bg-black/50 backdrop-blur-sm z-50 flex items-center justify-center p-4">
      <div 
        ref={modalRef}
        className={clsx(
          'w-full max-w-md rounded-xl shadow-2xl',
          isDark ? 'bg-gray-800 text-white' : 'bg-white text-gray-900'
        )}
        onKeyDown={handleKeyPress}
      >
        {/* Header */}
        <div className="flex items-center justify-between p-6 border-b border-gray-200 dark:border-gray-700">
          <div className="flex items-center gap-2">
            <File className="w-5 h-5" />
            <h2 className="text-lg font-semibold">Search Files</h2>
          </div>
          <button
            onClick={onClose}
            className="p-1 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors"
          >
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Form */}
        <form onSubmit={handleSubmit} className="p-6">
          {/* Query Input */}
          <div className="space-y-4">
            <div>
              <label className="block text-sm font-medium mb-2">
                Search Query
              </label>
              <div className="relative">
                <Search className="absolute left-3 top-1/2 transform -translate-y-1/2 w-4 h-4 text-gray-400" />
                <input
                  ref={queryInputRef}
                  type="text"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  placeholder="Enter keywords, function names, or concepts..."
                  className={clsx(
                    'w-full pl-10 pr-4 py-2 rounded-lg border transition-colors',
                    'focus:outline-none focus:ring-2 focus:ring-purple-500 focus:border-transparent',
                    isDark
                      ? 'bg-gray-700 border-gray-600 text-white placeholder-gray-400'
                      : 'bg-white border-gray-300 text-gray-900 placeholder-gray-500'
                  )}
                  required
                />
              </div>
            </div>

            {/* Advanced Filters Toggle */}
            <button
              type="button"
              onClick={() => setShowAdvanced(!showAdvanced)}
              className="flex items-center gap-2 text-sm text-purple-600 dark:text-purple-400 hover:underline"
            >
              <Filter className="w-4 h-4" />
              Advanced Filters
            </button>

            {/* Advanced Filters */}
            {showAdvanced && (
              <div className="space-y-4 pt-4 border-t border-gray-200 dark:border-gray-700">
                {/* File Extensions */}
                <div>
                  <label className="block text-sm font-medium mb-2">
                    File Extensions
                  </label>
                  <div className="flex gap-2 mb-2">
                    <input
                      type="text"
                      value={extensionInput}
                      onChange={(e) => setExtensionInput(e.target.value)}
                      onKeyPress={(e) => e.key === 'Enter' && (e.preventDefault(), addExtension())}
                      placeholder="e.g., ts, py, md"
                      className={clsx(
                        'flex-1 px-3 py-1 text-sm rounded border transition-colors',
                        isDark
                          ? 'bg-gray-700 border-gray-600 text-white placeholder-gray-400'
                          : 'bg-white border-gray-300 text-gray-900 placeholder-gray-500'
                      )}
                    />
                    <button
                      type="button"
                      onClick={addExtension}
                      className="px-3 py-1 text-sm bg-purple-600 text-white rounded hover:bg-purple-700 transition-colors"
                    >
                      Add
                    </button>
                  </div>
                  
                  {/* Common extensions */}
                  <div className="flex flex-wrap gap-1 mb-2">
                    {commonExtensions.map(ext => (
                      <button
                        key={ext}
                        type="button"
                        onClick={() => {
                          if (!filters.file_extensions?.includes(ext)) {
                            setFilters(prev => ({
                              ...prev,
                              file_extensions: [...(prev.file_extensions || []), ext]
                            }));
                          }
                        }}
                        className={clsx(
                          'px-2 py-1 text-xs rounded border transition-colors',
                          filters.file_extensions?.includes(ext)
                            ? 'bg-purple-100 border-purple-300 text-purple-800 cursor-default'
                            : isDark
                              ? 'bg-gray-700 border-gray-600 text-gray-300 hover:bg-gray-600'
                              : 'bg-gray-100 border-gray-300 text-gray-700 hover:bg-gray-200'
                        )}
                        disabled={filters.file_extensions?.includes(ext)}
                      >
                        {ext}
                      </button>
                    ))}
                  </div>

                  {/* Selected extensions */}
                  {filters.file_extensions && filters.file_extensions.length > 0 && (
                    <div className="flex flex-wrap gap-1">
                      {filters.file_extensions.map(ext => (
                        <span
                          key={ext}
                          className="inline-flex items-center gap-1 px-2 py-1 text-xs bg-purple-100 dark:bg-purple-900/30 text-purple-800 dark:text-purple-300 rounded"
                        >
                          {ext}
                          <button
                            type="button"
                            onClick={() => removeExtension(ext)}
                            className="hover:bg-purple-200 dark:hover:bg-purple-800 rounded-full p-0.5"
                          >
                            <X className="w-3 h-3" />
                          </button>
                        </span>
                      ))}
                    </div>
                  )}
                </div>

                {/* Max Results */}
                <div>
                  <label className="block text-sm font-medium mb-2">
                    Max Results
                  </label>
                  <select
                    value={filters.max_results}
                    onChange={(e) => setFilters(prev => ({ ...prev, max_results: parseInt(e.target.value) }))}
                    className={clsx(
                      'w-full px-3 py-2 rounded-lg border transition-colors',
                      isDark
                        ? 'bg-gray-700 border-gray-600 text-white'
                        : 'bg-white border-gray-300 text-gray-900'
                    )}
                  >
                    <option value={10}>10</option>
                    <option value={20}>20</option>
                    <option value={50}>50</option>
                    <option value={100}>100</option>
                  </select>
                </div>

                {/* Include Content */}
                <div className="flex items-center gap-2">
                  <input
                    type="checkbox"
                    id="include-content"
                    checked={filters.include_content}
                    onChange={(e) => setFilters(prev => ({ ...prev, include_content: e.target.checked }))}
                    className="w-4 h-4 text-purple-600 rounded focus:ring-purple-500"
                  />
                  <label htmlFor="include-content" className="text-sm">
                    Include file content in search
                  </label>
                </div>
              </div>
            )}
          </div>

          {/* Actions */}
          <div className="flex gap-3 pt-6">
            <button
              type="button"
              onClick={onClose}
              className={clsx(
                'flex-1 px-4 py-2 rounded-lg border transition-colors',
                isDark
                  ? 'border-gray-600 text-gray-300 hover:bg-gray-700'
                  : 'border-gray-300 text-gray-700 hover:bg-gray-50'
              )}
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={!query.trim()}
              className={clsx(
                'flex-1 px-4 py-2 rounded-lg transition-colors font-medium',
                !query.trim()
                  ? 'bg-gray-400 text-gray-600 cursor-not-allowed'
                  : 'bg-purple-600 text-white hover:bg-purple-700'
              )}
            >
              Search Files
            </button>
          </div>
        </form>
      </div>
    </div>
  );
};
