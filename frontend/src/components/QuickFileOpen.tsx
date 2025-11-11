// src/components/QuickFileOpen.tsx

import React, { useState, useEffect, useRef } from 'react';
import { Command, File, Folder, Search, X } from 'lucide-react';
import clsx from 'clsx';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useAppState } from '../stores/useAppState';

interface FileNode {
  name: string;
  path: string;
  type?: 'file' | 'directory';
  node_type?: 'File' | 'Directory';
  is_directory?: boolean;
  children?: FileNode[];
}

interface QuickFileOpenProps {
  isOpen: boolean;
  onClose: () => void;
}

export const QuickFileOpen: React.FC<QuickFileOpenProps> = ({
  isOpen,
  onClose,
}) => {
  const [query, setQuery] = useState('');
  const [files, setFiles] = useState<FileNode[]>([]);
  const [filteredFiles, setFilteredFiles] = useState<FileNode[]>([]);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [loading, setLoading] = useState(false);
  
  const inputRef = useRef<HTMLInputElement>(null);
  const modalRef = useRef<HTMLDivElement>(null);
  const send = useWebSocketStore(state => state.send);
  const subscribe = useWebSocketStore(state => state.subscribe);
  const { currentProject } = useAppState();
  
  // PERFORMANCE FIX: Extract projectId to stabilize dependencies
  const projectId = currentProject?.id;
  const projectName = currentProject?.name;

  // Subscribe to WebSocket messages
  useEffect(() => {
    const unsubscribe = subscribe('quick-file-open', (message) => {
      if (message.type !== 'data') return;
      
      const data = message.data;
      
      // ONLY handle git.tree responses here
      // file_content is handled by the global message handler
      if (data?.type === 'file_tree' && isOpen) {
        console.log('Git tree response received:', data);
        if (data.tree && Array.isArray(data.tree)) {
          const flatFiles = flattenFiles(data.tree);
          console.log('Processed files:', flatFiles.length);
          setFiles(flatFiles);
          setLoading(false);
        }
      }
    });
    
    return unsubscribe;
  }, [subscribe, isOpen]);

  // Load files when modal opens - PERFORMANCE FIX: Use projectId instead of currentProject
  useEffect(() => {
    console.log('QuickFileOpen: Modal state changed:', {
      isOpen,
      hasProject: !!projectId,
      projectId
    });
    
    if (isOpen && projectId) {
      loadProjectFiles();
    }
    
    if (isOpen && inputRef.current) {
      setTimeout(() => inputRef.current?.focus(), 100);
    }
    
    if (!isOpen) {
      setQuery('');
      setSelectedIndex(0);
      setLoading(false);
    }
  }, [isOpen, projectId]);  // ← CRITICAL FIX: Use projectId, not currentProject object

  const loadProjectFiles = async () => {
    if (!projectId) {
      console.log('No current project for file loading');
      return;
    }

    console.log('Loading files for project:', projectName);
    setLoading(true);
    
    try {
      await send({
        type: 'git_command',
        method: 'git.tree',
        params: { 
          project_id: projectId,
          recursive: true 
        }
      });
    } catch (error) {
      console.error('Failed to load project files:', error);
      setLoading(false);
    }
  };

  const flattenFiles = (nodes: FileNode[], prefix: string = ''): FileNode[] => {
    const result: FileNode[] = [];
    
    for (const node of nodes) {
      const fullPath = (node.path || node.name || '').replace(/\/+/, '/').replace(/\/+/g, '/');
      
      const isDirectory = node.is_directory || 
                         node.type === 'directory' || 
                         node.node_type === 'Directory' || 
                         (node.children && node.children.length > 0);
      
      if (!isDirectory) {
        result.push({
          name: node.name,
          path: fullPath,
          type: 'file'
        });
      }
      
      if (node.children && node.children.length > 0) {
        result.push(...flattenFiles(node.children, fullPath));
      }
    }
    
    return result;
  };

  // Filter files based on search query
  useEffect(() => {
    if (!query.trim()) {
      setFilteredFiles(files);
    } else {
      const filtered = files.filter(file => {
        const nameMatch = file.name.toLowerCase().includes(query.toLowerCase());
        const pathMatch = file.path.toLowerCase().includes(query.toLowerCase());
        return nameMatch || pathMatch;
      });
      setFilteredFiles(filtered);
    }
    setSelectedIndex(0);
  }, [query, files]);

  const handleFileSelect = async (file: FileNode) => {
    const normalizedPath = (file.path || '').replace(/\/+/, '/').replace(/\/+/g, '/');
    console.log('File selected:', normalizedPath);
    console.log('Project ID:', projectId);
    console.log('Requesting file content from backend...');
    
    try {
      const requestParams = {
        project_id: projectId,
        file_path: normalizedPath
      };
      
      console.log('Sending request with params:', JSON.stringify(requestParams, null, 2));
      
      await send({
        type: 'git_command',
        method: 'git.file',
        params: requestParams
      });
      
      console.log('File request sent');
      onClose();
      
    } catch (error) {
      console.error('Failed to request file:', error);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    switch (e.key) {
      case 'ArrowDown':
        e.preventDefault();
        setSelectedIndex(prev => Math.min(prev + 1, filteredFiles.length - 1));
        break;
        
      case 'ArrowUp':
        e.preventDefault();
        setSelectedIndex(prev => Math.max(prev - 1, 0));
        break;
        
      case 'Enter':
        e.preventDefault();
        if (filteredFiles[selectedIndex]) {
          handleFileSelect(filteredFiles[selectedIndex]);
        }
        break;
        
      case 'Escape':
        e.preventDefault();
        onClose();
        break;
    }
  };

  const handleOverlayClick = (e: React.MouseEvent) => {
    if (e.target === e.currentTarget) {
      onClose();
    }
  };

  if (!isOpen) return null;

  return (
    <div 
      className="fixed inset-0 bg-black bg-opacity-50 z-50 flex items-start justify-center pt-24"
      onClick={handleOverlayClick}
    >
      <div ref={modalRef} className="bg-slate-900 rounded-lg shadow-2xl border border-slate-700 w-full max-w-2xl max-h-[600px] flex flex-col">
        <div className="flex items-center px-4 py-3 border-b border-slate-700">
          <Search className="text-slate-400 mr-3" size={20} />
          <input
            ref={inputRef}
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder={currentProject ? "Type to search files..." : "Select a project first..."}
            className="flex-1 bg-transparent text-white outline-none placeholder-slate-500"
            disabled={!currentProject}
          />
          <button
            onClick={onClose}
            className="ml-3 text-slate-400 hover:text-white transition-colors"
          >
            <X size={20} />
          </button>
        </div>
        
        <div className="flex-1 overflow-y-auto">
          {!currentProject ? (
            <div className="px-4 py-8 text-center text-slate-500">
              Select a project to browse files
            </div>
          ) : loading ? (
            <div className="px-4 py-8 text-center text-slate-500">
              Loading files...
            </div>
          ) : filteredFiles.length === 0 ? (
            <div className="px-4 py-8 text-center text-slate-500">
              {query ? 'No files found' : 'No files in project'}
            </div>
          ) : (
            <ul>
              {filteredFiles.map((file, index) => (
                <li
                  key={file.path}
                  className={clsx(
                    'px-4 py-2 cursor-pointer flex items-center gap-2',
                    index === selectedIndex
                      ? 'bg-slate-700 text-slate-100'
                      : 'text-slate-300 hover:bg-slate-700/50'
                  )}
                  onMouseEnter={() => setSelectedIndex(index)}
                  onClick={() => handleFileSelect(file)}
                >
                  <File className="w-4 h-4 text-slate-500 flex-shrink-0" />
                  <div className="flex-1 overflow-hidden">
                    <div className="font-medium truncate">{file.name}</div>
                    <div className="text-xs text-slate-500 truncate">
                      {file.path}
                    </div>
                  </div>
                </li>
              ))}
            </ul>
          )}
        </div>

        <div className="p-2 border-t border-slate-700 text-xs text-slate-500 flex items-center justify-between">
          <div>
            {filteredFiles.length} {filteredFiles.length === 1 ? 'file' : 'files'}
          </div>
          <div className="flex items-center gap-2">
            <kbd className="px-1.5 py-0.5 bg-slate-700 rounded">↑↓</kbd> Navigate
            <kbd className="px-1.5 py-0.5 bg-slate-700 rounded">Enter</kbd> Open
            <kbd className="px-1.5 py-0.5 bg-slate-700 rounded">Esc</kbd> Close
          </div>
        </div>
      </div>
    </div>
  );
};

export const useQuickFileOpen = () => {
  const [isOpen, setIsOpen] = useState(false);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 'p') {
        e.preventDefault();
        setIsOpen(true);
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, []);

  return {
    isOpen,
    open: () => setIsOpen(true),
    close: () => setIsOpen(false),
  };
};
