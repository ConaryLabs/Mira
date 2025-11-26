// frontend/src/components/SemanticSearch.tsx
// Semantic code search component with results display

import React, { useState, useCallback } from 'react';
import { Search, FileCode, Clock, AlertCircle, X, Loader2 } from 'lucide-react';
import { useCodeSearch, useCodeIntelligenceStore, CodeSearchResult } from '../stores/useCodeIntelligenceStore';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useAppState } from '../stores/useAppState';

interface SearchResultItemProps {
  result: CodeSearchResult;
  onSelect: (result: CodeSearchResult) => void;
}

function SearchResultItem({ result, onSelect }: SearchResultItemProps) {
  // Extract filename from path
  const fileName = result.filePath.split('/').pop() || result.filePath;
  const directory = result.filePath.replace(fileName, '').replace(/\/$/, '');

  return (
    <button
      onClick={() => onSelect(result)}
      className="w-full text-left p-3 hover:bg-slate-700/50 border-b border-slate-700/50 transition-colors"
    >
      <div className="flex items-start gap-2">
        <FileCode className="w-4 h-4 text-blue-400 mt-0.5 flex-shrink-0" />
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <span className="text-sm font-medium text-slate-200 truncate">
              {fileName}
            </span>
            <span className="text-xs text-slate-500 truncate">
              {directory}
            </span>
          </div>
          <div className="mt-1 text-xs text-slate-400">
            Lines {result.lineStart}-{result.lineEnd}
            {result.language && (
              <span className="ml-2 px-1.5 py-0.5 bg-slate-700 rounded text-slate-300">
                {result.language}
              </span>
            )}
          </div>
          {/* Code Preview */}
          <pre className="mt-2 p-2 bg-slate-900 rounded text-xs text-slate-300 overflow-x-auto">
            <code>{result.content.slice(0, 200)}{result.content.length > 200 ? '...' : ''}</code>
          </pre>
          {/* Score indicator */}
          <div className="mt-1 flex items-center gap-1">
            <div className="h-1 w-16 bg-slate-700 rounded-full overflow-hidden">
              <div
                className="h-full bg-blue-500 rounded-full"
                style={{ width: `${Math.min(result.score * 100, 100)}%` }}
              />
            </div>
            <span className="text-xs text-slate-500">
              {(result.score * 100).toFixed(0)}% match
            </span>
          </div>
        </div>
      </div>
    </button>
  );
}

export function SemanticSearch() {
  const { query, results, isSearching, error, setQuery, clearSearch } = useCodeSearch();
  const setSearchResults = useCodeIntelligenceStore((state) => state.setSearchResults);
  const setSearching = useCodeIntelligenceStore((state) => state.setSearching);
  const setSearchError = useCodeIntelligenceStore((state) => state.setSearchError);

  const send = useWebSocketStore((state) => state.send);
  const currentProject = useAppState((state) => state.currentProject);

  const [inputValue, setInputValue] = useState(query);

  const handleSearch = useCallback(async () => {
    if (!inputValue.trim()) return;

    setQuery(inputValue);
    setSearching(true);
    setSearchError(null);

    try {
      // Send search request via WebSocket
      await send({
        type: 'code_intelligence_command',
        method: 'code.semantic_search',
        params: {
          query: inputValue,
          project_id: currentProject?.id,
          limit: 10,
        },
      });
    } catch (err) {
      setSearchError('Failed to send search request');
    }
  }, [inputValue, setQuery, setSearching, setSearchError, send, currentProject]);

  const handleKeyPress = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      handleSearch();
    }
  };

  const handleResultSelect = (result: CodeSearchResult) => {
    // Open file in artifact viewer or editor
    // This will be wired up to the artifact system
    console.log('Selected result:', result.filePath);
  };

  const handleClear = () => {
    setInputValue('');
    clearSearch();
  };

  return (
    <div className="flex flex-col h-full">
      {/* Search Input */}
      <div className="p-3 border-b border-slate-700">
        <div className="relative">
          <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-slate-400" />
          <input
            type="text"
            value={inputValue}
            onChange={(e) => setInputValue(e.target.value)}
            onKeyPress={handleKeyPress}
            placeholder="Search code semantically..."
            className="w-full pl-9 pr-8 py-2 bg-slate-800 border border-slate-700 rounded-lg text-sm text-slate-200 placeholder-slate-500 focus:outline-none focus:border-blue-500 transition-colors"
          />
          {inputValue && (
            <button
              onClick={handleClear}
              className="absolute right-3 top-1/2 -translate-y-1/2 text-slate-400 hover:text-slate-200"
            >
              <X className="w-4 h-4" />
            </button>
          )}
        </div>
        {!currentProject && (
          <p className="mt-2 text-xs text-yellow-400">
            Select a project to enable semantic search
          </p>
        )}
      </div>

      {/* Results Area */}
      <div className="flex-1 overflow-y-auto">
        {isSearching && (
          <div className="flex items-center justify-center gap-2 p-8 text-slate-400">
            <Loader2 className="w-5 h-5 animate-spin" />
            <span className="text-sm">Searching...</span>
          </div>
        )}

        {error && (
          <div className="p-4">
            <div className="flex items-center gap-2 text-red-400">
              <AlertCircle className="w-4 h-4" />
              <span className="text-sm">{error}</span>
            </div>
          </div>
        )}

        {!isSearching && !error && results.length === 0 && query && (
          <div className="flex flex-col items-center justify-center p-8 text-center">
            <Search className="w-8 h-8 text-slate-600 mb-2" />
            <p className="text-sm text-slate-400">No results found</p>
            <p className="text-xs text-slate-500 mt-1">
              Try different keywords or check your project selection
            </p>
          </div>
        )}

        {!isSearching && !error && results.length === 0 && !query && (
          <div className="flex flex-col items-center justify-center p-8 text-center">
            <Search className="w-8 h-8 text-slate-600 mb-2" />
            <p className="text-sm text-slate-400">Semantic Code Search</p>
            <p className="text-xs text-slate-500 mt-1">
              Search your codebase using natural language queries
            </p>
          </div>
        )}

        {!isSearching && results.length > 0 && (
          <div>
            <div className="px-3 py-2 text-xs text-slate-500 border-b border-slate-700/50">
              {results.length} result{results.length !== 1 ? 's' : ''} found
            </div>
            {results.map((result) => (
              <SearchResultItem
                key={result.id}
                result={result}
                onSelect={handleResultSelect}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
