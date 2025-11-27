// frontend/src/components/FileBrowser.tsx
// Enhanced file browser with semantic tags showing code intelligence data

import React, { useState, useEffect, useCallback } from 'react';
import {
  ChevronRight,
  ChevronDown,
  File,
  Folder,
  AlertTriangle,
  TestTube,
  Zap,
  Code2,
  Eye,
  EyeOff,
  RefreshCw,
} from 'lucide-react';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useAppState } from '../stores/useAppState';

interface FileNode {
  name: string;
  path: string;
  is_directory: boolean;
  children?: FileNode[];
}

interface FileSemanticStats {
  file_path: string;
  language: string | null;
  element_count: number;
  complexity_score: number | null;
  quality_issue_count: number;
  is_test_file: boolean;
  is_analyzed: boolean;
  function_count: number;
  line_count: number;
}

// Color coding for complexity
function getComplexityColor(score: number | null): string {
  if (score === null) return '';
  if (score >= 30) return 'text-red-400';
  if (score >= 15) return 'text-yellow-400';
  if (score >= 5) return 'text-green-400';
  return 'text-slate-500';
}

// Get language icon color
function getLanguageColor(lang: string | null): string {
  if (!lang) return 'text-slate-400';
  switch (lang.toLowerCase()) {
    case 'rust':
      return 'text-orange-400';
    case 'typescript':
    case 'javascript':
      return 'text-yellow-400';
    case 'python':
      return 'text-blue-400';
    default:
      return 'text-slate-400';
  }
}

interface SemanticTagsProps {
  stats: FileSemanticStats;
  showTags: boolean;
}

function SemanticTags({ stats, showTags }: SemanticTagsProps) {
  if (!showTags) return null;

  return (
    <div className="flex items-center gap-1 ml-auto">
      {/* Test file indicator */}
      {stats.is_test_file && (
        <span className="text-cyan-400" title="Test file">
          <TestTube size={12} />
        </span>
      )}

      {/* Quality issues indicator */}
      {stats.quality_issue_count > 0 && (
        <span
          className="text-yellow-400 text-xs font-mono"
          title={`${stats.quality_issue_count} quality issue${stats.quality_issue_count > 1 ? 's' : ''}`}
        >
          <AlertTriangle size={12} />
        </span>
      )}

      {/* Complexity indicator */}
      {stats.complexity_score !== null && stats.complexity_score > 0 && (
        <span
          className={`text-xs font-mono ${getComplexityColor(stats.complexity_score)}`}
          title={`Complexity: ${stats.complexity_score.toFixed(1)}`}
        >
          <Zap size={12} />
        </span>
      )}

      {/* Element count */}
      {stats.element_count > 0 && (
        <span
          className="text-slate-500 text-xs font-mono"
          title={`${stats.element_count} element${stats.element_count > 1 ? 's' : ''} (${stats.function_count} function${stats.function_count > 1 ? 's' : ''})`}
        >
          {stats.element_count}
        </span>
      )}

      {/* Analyzed indicator */}
      {stats.is_analyzed && (
        <span className="text-green-500" title="AST analyzed">
          <Code2 size={10} />
        </span>
      )}
    </div>
  );
}

export const FileBrowser: React.FC = () => {
  const [fileTree, setFileTree] = useState<FileNode[]>([]);
  const [expandedPaths, setExpandedPaths] = useState<Set<string>>(new Set());
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [fileContent, setFileContent] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [semanticStats, setSemanticStats] = useState<Map<string, FileSemanticStats>>(new Map());
  const [showSemanticTags, setShowSemanticTags] = useState(true);
  const [loadingStats, setLoadingStats] = useState(false);

  const send = useWebSocketStore((state) => state.send);
  const subscribe = useWebSocketStore((state) => state.subscribe);
  const { currentProject } = useAppState();

  // Load file tree when project changes
  useEffect(() => {
    if (currentProject) {
      loadFileTree();
      loadSemanticStats();
    }
  }, [currentProject]);

  // Handle WebSocket responses
  useEffect(() => {
    const unsubscribe = subscribe('file-browser', (message) => {
      if (message.type === 'data' && message.data) {
        const data = message.data;

        if (data.type === 'file_tree') {
          console.log('File tree received:', data.tree);
          setFileTree(data.tree || []);
        }

        if (data.type === 'file_content') {
          console.log('File content received for:', data.path);
          setFileContent(data.content);
          setLoading(false);
        }

        if (data.type === 'file_semantic_stats') {
          console.log('Semantic stats received:', data.files?.length || 0, 'files');
          const statsMap = new Map<string, FileSemanticStats>();
          (data.files || []).forEach((f: FileSemanticStats) => {
            statsMap.set(f.file_path, f);
          });
          setSemanticStats(statsMap);
          setLoadingStats(false);
        }
      }
    });

    return unsubscribe;
  }, [subscribe]);

  const loadFileTree = async () => {
    if (!currentProject) return;

    try {
      await send({
        type: 'git_command',
        method: 'git.tree',
        params: { project_id: currentProject.id },
      });
    } catch (error) {
      console.error('Failed to load file tree:', error);
    }
  };

  const loadSemanticStats = useCallback(async () => {
    if (!currentProject) return;

    setLoadingStats(true);
    try {
      await send({
        type: 'code_intelligence_command',
        method: 'code.file_semantic_stats',
        params: { project_id: currentProject.id },
      });
    } catch (error) {
      console.error('Failed to load semantic stats:', error);
      setLoadingStats(false);
    }
  }, [send, currentProject]);

  const toggleExpanded = (path: string) => {
    const newExpanded = new Set(expandedPaths);
    if (newExpanded.has(path)) {
      newExpanded.delete(path);
    } else {
      newExpanded.add(path);
    }
    setExpandedPaths(newExpanded);
  };

  const selectFile = async (path: string) => {
    if (!currentProject || selectedFile === path) return;

    setSelectedFile(path);
    setFileContent(null);
    setLoading(true);

    try {
      await send({
        type: 'git_command',
        method: 'git.file',
        params: {
          project_id: currentProject.id,
          file_path: path,
        },
      });
    } catch (error) {
      console.error('Failed to load file:', error);
      setLoading(false);
    }
  };

  const renderFileNode = (node: FileNode, depth: number = 0): React.ReactNode => {
    const isExpanded = expandedPaths.has(node.path);
    const isSelected = selectedFile === node.path;
    const stats = semanticStats.get(node.path);

    return (
      <div key={node.path}>
        <div
          className={`flex items-center gap-1 py-1 px-2 hover:bg-slate-800 cursor-pointer text-sm ${
            isSelected ? 'bg-blue-600/20 text-blue-300' : 'text-slate-300'
          }`}
          style={{ paddingLeft: `${depth * 16 + 8}px` }}
          onClick={() => {
            if (node.is_directory) {
              toggleExpanded(node.path);
            } else {
              selectFile(node.path);
            }
          }}
        >
          {node.is_directory ? (
            <>
              {isExpanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
              <Folder size={14} className="text-blue-400" />
            </>
          ) : (
            <>
              <span style={{ width: '12px' }} />
              <File
                size={14}
                className={stats ? getLanguageColor(stats.language) : 'text-slate-400'}
              />
            </>
          )}
          <span className="truncate flex-1">{node.name}</span>
          {!node.is_directory && stats && (
            <SemanticTags stats={stats} showTags={showSemanticTags} />
          )}
        </div>

        {node.is_directory && isExpanded && node.children && (
          <div>
            {node.children.map((child) => renderFileNode(child, depth + 1))}
          </div>
        )}
      </div>
    );
  };

  if (!currentProject) {
    return (
      <div className="p-4 text-slate-500 text-sm">Select a project to browse files</div>
    );
  }

  if (fileTree.length === 0) {
    return (
      <div className="p-4 text-slate-500 text-sm">
        <div>No repository attached</div>
        <button
          onClick={loadFileTree}
          className="mt-2 text-blue-400 hover:text-blue-300"
        >
          Refresh
        </button>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col">
      {/* Toolbar */}
      <div className="flex-shrink-0 flex items-center justify-between px-2 py-1.5 border-b border-slate-700 bg-slate-850">
        <span className="text-sm font-medium text-slate-300">Files</span>
        <div className="flex items-center gap-1">
          {/* Toggle semantic tags */}
          <button
            onClick={() => setShowSemanticTags(!showSemanticTags)}
            className={`p-1 rounded transition-colors ${
              showSemanticTags
                ? 'text-blue-400 hover:bg-slate-700'
                : 'text-slate-500 hover:bg-slate-700'
            }`}
            title={showSemanticTags ? 'Hide semantic tags' : 'Show semantic tags'}
          >
            {showSemanticTags ? <Eye size={14} /> : <EyeOff size={14} />}
          </button>
          {/* Refresh button */}
          <button
            onClick={() => {
              loadFileTree();
              loadSemanticStats();
            }}
            className="p-1 text-slate-400 hover:text-slate-200 hover:bg-slate-700 rounded transition-colors"
            title="Refresh"
          >
            <RefreshCw size={14} className={loadingStats ? 'animate-spin' : ''} />
          </button>
        </div>
      </div>

      {/* Legend */}
      {showSemanticTags && (
        <div className="flex-shrink-0 flex items-center gap-3 px-2 py-1 border-b border-slate-700/50 text-xs text-slate-500">
          <span className="flex items-center gap-1">
            <TestTube size={10} className="text-cyan-400" /> Test
          </span>
          <span className="flex items-center gap-1">
            <AlertTriangle size={10} className="text-yellow-400" /> Issues
          </span>
          <span className="flex items-center gap-1">
            <Zap size={10} className="text-red-400" /> Complex
          </span>
          <span className="flex items-center gap-1">
            <Code2 size={10} className="text-green-500" /> Analyzed
          </span>
        </div>
      )}

      {/* Content */}
      <div className="flex flex-1 overflow-hidden">
        {/* File Tree */}
        <div className="w-1/3 border-r border-slate-700 overflow-y-auto">
          <div className="py-1">{fileTree.map((node) => renderFileNode(node))}</div>
        </div>

        {/* File Content */}
        <div className="flex-1 overflow-y-auto">
          {loading ? (
            <div className="p-4 text-slate-500">Loading...</div>
          ) : selectedFile && fileContent ? (
            <div className="h-full flex flex-col">
              <div className="flex-shrink-0 p-2 border-b border-slate-700 text-sm text-slate-400 flex items-center justify-between">
                <span className="truncate">{selectedFile}</span>
                {semanticStats.get(selectedFile) && (
                  <div className="flex items-center gap-2 text-xs">
                    {semanticStats.get(selectedFile)?.line_count && (
                      <span className="text-slate-500">
                        {semanticStats.get(selectedFile)!.line_count} lines
                      </span>
                    )}
                    {semanticStats.get(selectedFile)?.function_count !== undefined &&
                      semanticStats.get(selectedFile)!.function_count > 0 && (
                        <span className="text-slate-500">
                          {semanticStats.get(selectedFile)!.function_count} functions
                        </span>
                      )}
                    {semanticStats.get(selectedFile)?.complexity_score !== null && (
                      <span
                        className={getComplexityColor(
                          semanticStats.get(selectedFile)!.complexity_score
                        )}
                      >
                        Complexity: {semanticStats.get(selectedFile)!.complexity_score?.toFixed(1)}
                      </span>
                    )}
                  </div>
                )}
              </div>
              <pre className="flex-1 p-4 text-sm text-slate-300 overflow-auto">
                <code>{fileContent}</code>
              </pre>
            </div>
          ) : (
            <div className="p-4 text-slate-500 text-sm">
              Select a file to view its contents
            </div>
          )}
        </div>
      </div>
    </div>
  );
};
