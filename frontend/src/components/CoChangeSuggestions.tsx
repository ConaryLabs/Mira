// frontend/src/components/CoChangeSuggestions.tsx
// Component showing files that are often changed together with the current file

import React, { useEffect, useCallback, useState } from 'react';
import { GitBranch, FileCode, Loader2, Clock, TrendingUp, Search } from 'lucide-react';
import { useCoChangeSuggestions, CoChangeSuggestion, useCodeIntelligenceStore } from '../stores/useCodeIntelligenceStore';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useAppState } from '../stores/useAppState';

interface SuggestionItemProps {
  suggestion: CoChangeSuggestion;
  onSelect: (filePath: string) => void;
}

function SuggestionItem({ suggestion, onSelect }: SuggestionItemProps) {
  // Extract filename from path
  const fileName = suggestion.filePath.split('/').pop() || suggestion.filePath;
  const directory = suggestion.filePath.replace(fileName, '').replace(/\/$/, '');

  // Confidence color
  const getConfidenceColor = (confidence: number) => {
    if (confidence >= 0.8) return 'text-green-400';
    if (confidence >= 0.5) return 'text-yellow-400';
    return 'text-slate-400';
  };

  const getConfidenceBg = (confidence: number) => {
    if (confidence >= 0.8) return 'bg-green-500';
    if (confidence >= 0.5) return 'bg-yellow-500';
    return 'bg-slate-500';
  };

  return (
    <button
      onClick={() => onSelect(suggestion.filePath)}
      className="w-full text-left p-3 hover:bg-slate-700/50 border-b border-slate-700/50 transition-colors"
    >
      <div className="flex items-start gap-2">
        <FileCode className="w-4 h-4 text-purple-400 mt-0.5 flex-shrink-0" />
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <span className="text-sm font-medium text-slate-200 truncate">
              {fileName}
            </span>
          </div>
          {directory && (
            <div className="text-xs text-slate-500 truncate mt-0.5">
              {directory}
            </div>
          )}

          {/* Reason */}
          <p className="mt-1.5 text-xs text-slate-400">
            {suggestion.reason}
          </p>

          {/* Stats */}
          <div className="mt-2 flex items-center gap-3 text-xs">
            {/* Confidence bar */}
            <div className="flex items-center gap-1.5">
              <div className="h-1.5 w-12 bg-slate-700 rounded-full overflow-hidden">
                <div
                  className={`h-full rounded-full ${getConfidenceBg(suggestion.confidence)}`}
                  style={{ width: `${suggestion.confidence * 100}%` }}
                />
              </div>
              <span className={getConfidenceColor(suggestion.confidence)}>
                {(suggestion.confidence * 100).toFixed(0)}%
              </span>
            </div>

            {/* Co-change count */}
            <div className="flex items-center gap-1 text-slate-500">
              <TrendingUp className="w-3 h-3" />
              <span>{suggestion.coChangeCount} times</span>
            </div>

            {/* Last changed */}
            {suggestion.lastChanged && (
              <div className="flex items-center gap-1 text-slate-500">
                <Clock className="w-3 h-3" />
                <span>{formatRelativeTime(suggestion.lastChanged)}</span>
              </div>
            )}
          </div>
        </div>
      </div>
    </button>
  );
}

function formatRelativeTime(dateString: string): string {
  const date = new Date(dateString);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffDays = Math.floor(diffMs / (1000 * 60 * 60 * 24));

  if (diffDays === 0) return 'today';
  if (diffDays === 1) return 'yesterday';
  if (diffDays < 7) return `${diffDays} days ago`;
  if (diffDays < 30) return `${Math.floor(diffDays / 7)} weeks ago`;
  if (diffDays < 365) return `${Math.floor(diffDays / 30)} months ago`;
  return `${Math.floor(diffDays / 365)} years ago`;
}

export function CoChangeSuggestions() {
  const { suggestions, isLoading, currentFile } = useCoChangeSuggestions();
  const setCurrentFile = useCodeIntelligenceStore((state) => state.setCurrentFile);
  const setLoadingCoChange = useCodeIntelligenceStore((state) => state.setLoadingCoChange);
  const send = useWebSocketStore((state) => state.send);
  const { currentProject } = useAppState();
  const [inputPath, setInputPath] = useState('');

  const requestCoChangeData = useCallback(async (filePath: string) => {
    if (!filePath || !currentProject?.id) return;

    setLoadingCoChange(true);
    try {
      await send({
        type: 'code_intelligence_command',
        method: 'code.cochange',
        params: {
          project_id: currentProject.id,
          file_path: filePath,
        },
      });
    } catch (err) {
      console.error('Failed to request co-change data:', err);
      setLoadingCoChange(false);
    }
  }, [send, currentProject?.id, setLoadingCoChange]);

  const handleSearch = () => {
    if (inputPath.trim()) {
      setCurrentFile(inputPath.trim());
      requestCoChangeData(inputPath.trim());
    }
  };

  const handleSelect = (filePath: string) => {
    // Set the selected file as the new current file and fetch its suggestions
    setCurrentFile(filePath);
    requestCoChangeData(filePath);
  };

  if (isLoading) {
    return (
      <div className="flex items-center justify-center gap-2 p-8 text-slate-400">
        <Loader2 className="w-5 h-5 animate-spin" />
        <span className="text-sm">Loading suggestions...</span>
      </div>
    );
  }

  if (!currentFile) {
    return (
      <div className="flex flex-col items-center justify-center p-8 text-center">
        <GitBranch className="w-8 h-8 text-slate-600 mb-2" />
        <p className="text-sm text-slate-400">Co-Change Analysis</p>
        <p className="text-xs text-slate-500 mt-1 mb-4">
          Enter a file path to see files that are often changed together
        </p>
        {/* File path input */}
        <div className="w-full max-w-xs flex gap-2">
          <input
            type="text"
            value={inputPath}
            onChange={(e) => setInputPath(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && handleSearch()}
            placeholder="src/lib/example.rs"
            className="flex-1 px-3 py-2 bg-slate-800 border border-slate-600 rounded text-sm text-slate-200 placeholder-slate-500 focus:outline-none focus:border-purple-500"
          />
          <button
            onClick={handleSearch}
            disabled={!inputPath.trim() || !currentProject?.id}
            className="px-3 py-2 bg-purple-600 hover:bg-purple-700 disabled:bg-slate-700 disabled:cursor-not-allowed rounded text-sm text-white transition-colors"
          >
            <Search className="w-4 h-4" />
          </button>
        </div>
        {!currentProject?.id && (
          <p className="text-xs text-amber-400 mt-2">
            Select a project first
          </p>
        )}
      </div>
    );
  }

  if (suggestions.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center p-8 text-center">
        <GitBranch className="w-8 h-8 text-slate-600 mb-2" />
        <p className="text-sm text-slate-400">No co-change patterns found</p>
        <p className="text-xs text-slate-500 mt-1">
          This file doesn't have established co-change patterns yet
        </p>
      </div>
    );
  }

  // Extract current file name for display
  const currentFileName = currentFile.split('/').pop() || currentFile;

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="p-3 border-b border-slate-700">
        <div className="flex items-center gap-2">
          <GitBranch className="w-4 h-4 text-purple-400" />
          <span className="text-sm text-slate-200">Files often changed with</span>
        </div>
        <div className="mt-1 text-xs text-slate-400 truncate" title={currentFile}>
          {currentFileName}
        </div>
      </div>

      {/* Suggestions List */}
      <div className="flex-1 overflow-y-auto">
        <div className="px-3 py-2 text-xs text-slate-500 border-b border-slate-700/50">
          {suggestions.length} suggestion{suggestions.length !== 1 ? 's' : ''}
        </div>
        {suggestions.map((suggestion, index) => (
          <SuggestionItem
            key={`${suggestion.filePath}-${index}`}
            suggestion={suggestion}
            onSelect={handleSelect}
          />
        ))}
      </div>

      {/* Info Footer */}
      <div className="p-3 border-t border-slate-700 bg-slate-800/50">
        <p className="text-xs text-slate-500">
          Based on git history analysis. Higher confidence means files are more
          frequently changed together.
        </p>
      </div>
    </div>
  );
}
