// src/components/FileBrowser.tsx
import React, { useState, useEffect } from 'react';
import { ChevronRight, ChevronDown, File, Folder } from 'lucide-react';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useAppState } from '../stores/useAppState';

interface FileNode {
  name: string;
  path: string;
  is_directory: boolean;
  children?: FileNode[];
}

export const FileBrowser: React.FC = () => {
  const [fileTree, setFileTree] = useState<FileNode[]>([]);
  const [expandedPaths, setExpandedPaths] = useState<Set<string>>(new Set());
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [fileContent, setFileContent] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  
  const send = useWebSocketStore(state => state.send);
  const subscribe = useWebSocketStore(state => state.subscribe);
  const { currentProject } = useAppState();

  // Load file tree when project changes
  useEffect(() => {
    if (currentProject) {
      loadFileTree();
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
        params: { project_id: currentProject.id }
      });
    } catch (error) {
      console.error('Failed to load file tree:', error);
    }
  };

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
          file_path: path 
        }
      });
    } catch (error) {
      console.error('Failed to load file:', error);
      setLoading(false);
    }
  };

  const renderFileNode = (node: FileNode, depth: number = 0): React.ReactNode => {
    const isExpanded = expandedPaths.has(node.path);
    const isSelected = selectedFile === node.path;
    
    return (
      <div key={node.path}>
        <div
          className={`flex items-center gap-1 py-1 px-2 hover:bg-gray-800 cursor-pointer text-sm ${
            isSelected ? 'bg-blue-600/20 text-blue-300' : 'text-gray-300'
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
              <File size={14} className="text-gray-400" />
            </>
          )}
          <span className="truncate">{node.name}</span>
        </div>
        
        {node.is_directory && isExpanded && node.children && (
          <div>
            {node.children.map(child => renderFileNode(child, depth + 1))}
          </div>
        )}
      </div>
    );
  };

  if (!currentProject) {
    return (
      <div className="p-4 text-gray-500 text-sm">
        Select a project to browse files
      </div>
    );
  }

  if (fileTree.length === 0) {
    return (
      <div className="p-4 text-gray-500 text-sm">
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
    <div className="flex h-full">
      {/* File Tree */}
      <div className="w-1/3 border-r border-gray-700 overflow-y-auto">
        <div className="p-2 border-b border-gray-700 text-sm font-medium text-gray-300">
          Files
        </div>
        <div className="py-1">
          {fileTree.map(node => renderFileNode(node))}
        </div>
      </div>
      
      {/* File Content */}
      <div className="flex-1 overflow-y-auto">
        {loading ? (
          <div className="p-4 text-gray-500">Loading...</div>
        ) : selectedFile && fileContent ? (
          <div className="h-full">
            <div className="p-2 border-b border-gray-700 text-sm text-gray-400">
              {selectedFile}
            </div>
            <pre className="p-4 text-sm text-gray-300 overflow-auto">
              <code>{fileContent}</code>
            </pre>
          </div>
        ) : (
          <div className="p-4 text-gray-500 text-sm">
            Select a file to view its contents
          </div>
        )}
      </div>
    </div>
  );
};
