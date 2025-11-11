// src/components/documents/DocumentSearch.tsx
import { useState, useEffect, useCallback } from 'react';
import { useWebSocketStore } from '../../stores/useWebSocketStore';
import { Search, FileText, Sparkles } from 'lucide-react';
import type { DocumentSearchResult } from '../../types';

interface DocumentSearchProps {
  projectId: string;
}

export function DocumentSearch({ projectId }: DocumentSearchProps) {
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<DocumentSearchResult[]>([]);
  const [searching, setSearching] = useState(false);
  const [hasSearched, setHasSearched] = useState(false);
  const { send, subscribe } = useWebSocketStore();

  useEffect(() => {
    const unsubscribe = subscribe('doc-search', (message) => {
      if (message.type === 'data' && message.data?.type === 'document_search_results') {
        setResults(message.data.results || []);
        setSearching(false);
        setHasSearched(true);
      }
      
      // Handle search errors
      if (message.type === 'error' && searching) {
        console.error('Search error:', message.message);
        setSearching(false);
        setHasSearched(true);
      }
    });
    
    return unsubscribe;
  }, [subscribe, searching]);

  const handleSearch = useCallback(async () => {
    if (!query.trim()) return;
    
    setSearching(true);
    setHasSearched(false);
    
    try {
      await send({
        type: 'document_command',
        method: 'documents.search',
        params: {
          project_id: projectId,
          query: query.trim(),
          limit: 10,
        },
      });
    } catch (error) {
      console.error('Search failed:', error);
      setSearching(false);
      setHasSearched(true);
    }
  }, [projectId, query, send]);

  const handleKeyPress = useCallback((e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !searching && query.trim()) {
      handleSearch();
    }
  }, [handleSearch, searching, query]);

  const getScoreColor = (score: number): string => {
    if (score >= 0.8) return 'text-green-500';
    if (score >= 0.6) return 'text-blue-500';
    if (score >= 0.4) return 'text-yellow-500';
    return 'text-slate-500';
  };

  const getScoreLabel = (score: number): string => {
    if (score >= 0.8) return 'Excellent match';
    if (score >= 0.6) return 'Good match';
    if (score >= 0.4) return 'Fair match';
    return 'Weak match';
  };

  const highlightQuery = (text: string, query: string): React.ReactNode => {
    if (!query.trim()) return text;
    
    const parts = text.split(new RegExp(`(${query})`, 'gi'));
    return parts.map((part, i) => 
      part.toLowerCase() === query.toLowerCase()
        ? <span key={i} className="bg-yellow-500/30 text-yellow-200 font-semibold">{part}</span>
        : part
    );
  };

  return (
    <div className="space-y-4">
      {/* Search Input */}
      <div>
        <div className="flex gap-2">
          <input
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyPress={handleKeyPress}
            placeholder="Search across all documents..."
            className="flex-1 px-4 py-3 bg-slate-900 border border-slate-600 rounded-lg text-slate-100 placeholder-slate-500 focus:outline-none focus:border-blue-500"
          />
          <button
            onClick={handleSearch}
            disabled={searching || !query.trim()}
            className="px-6 py-3 bg-blue-600 hover:bg-blue-700 disabled:bg-slate-700 disabled:text-slate-500 rounded-lg text-white font-medium transition-colors"
          >
            {searching ? 'Searching...' : 'Search'}
          </button>
        </div>
        <p className="text-xs text-slate-500 mt-2">
          Semantic search finds relevant content based on meaning, not just keywords
        </p>
      </div>

      {/* Search Results */}
      {!hasSearched ? (
        <div className="flex flex-col items-center justify-center py-12 text-slate-400">
          <Search className="w-16 h-16 mb-4 text-slate-600" />
          <p>Enter a query to search your documents</p>
        </div>
      ) : results.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-12 text-slate-400">
          <Search className="w-16 h-16 mb-4 text-slate-600" />
          <p>No results found for "{query}"</p>
          <p className="text-sm mt-2">Try different keywords or a broader query</p>
        </div>
      ) : (
        <div className="space-y-4">
          <div className="text-sm text-slate-400 mb-4">
            Found {results.length} relevant {results.length === 1 ? 'result' : 'results'}
          </div>
          
          {results.map((result, index) => (
            <div
              key={`${result.chunk_id}-${index}`}
              className="p-4 bg-slate-700/50 rounded-lg border border-slate-600 hover:border-slate-500 transition-colors"
            >
              {/* Result Header */}
              <div className="flex items-start justify-between mb-3">
                <div className="flex items-center gap-2">
                  <FileText className="w-4 h-4 text-blue-400 flex-shrink-0" />
                  <span className="font-medium text-slate-100">{result.file_name}</span>
                  {result.page_number && (
                    <span className="text-xs text-slate-500">• Page {result.page_number}</span>
                  )}
                </div>
                
                {/* Relevance Score */}
                <div className="flex items-center gap-2">
                  <div className="w-24 h-2 bg-slate-600 rounded-full overflow-hidden">
                    <div
                      className={`h-full transition-all ${
                        result.score >= 0.8 ? 'bg-green-500' :
                        result.score >= 0.6 ? 'bg-blue-500' :
                        result.score >= 0.4 ? 'bg-yellow-500' :
                        'bg-slate-500'
                      }`}
                      style={{ width: `${result.score * 100}%` }}
                    />
                  </div>
                  <span className={`text-xs ${getScoreColor(result.score)} font-medium`}>
                    {getScoreLabel(result.score)}
                  </span>
                </div>
              </div>

              {/* Content Preview */}
              <div className="text-sm text-slate-300 leading-relaxed">
                {highlightQuery(result.content, query)}
              </div>

              {/* Chunk Info */}
              <div className="flex items-center gap-2 mt-3 text-xs text-slate-500">
                <Sparkles className="w-3 h-3" />
                <span>Chunk {result.chunk_index + 1}</span>
                <span>•</span>
                <span>Score: {(result.score * 100).toFixed(1)}%</span>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
