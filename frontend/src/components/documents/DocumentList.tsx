// src/components/documents/DocumentList.tsx
import { useState, useEffect, useCallback } from 'react';
import { useWebSocketStore } from '../../stores/useWebSocketStore';
import { FileText, Trash2, Download, Calendar } from 'lucide-react';
import type { DocumentMetadata } from '../../types';

interface DocumentListProps {
  projectId: string;
}

export function DocumentList({ projectId }: DocumentListProps) {
  const [documents, setDocuments] = useState<DocumentMetadata[]>([]);
  const [loading, setLoading] = useState(true);
  const [deleting, setDeleting] = useState<string | null>(null);
  const { send, subscribe } = useWebSocketStore();

  // Load documents on mount and when projectId changes
  useEffect(() => {
    setLoading(true);
    
    send({
      type: 'document_command',
      method: 'documents.list',
      params: { project_id: projectId },
    });

    const unsubscribe = subscribe('doc-list', (message) => {
      if (message.type === 'data' && message.data?.type === 'document_list') {
        setDocuments(message.data.documents || []);
        setLoading(false);
      }
    });
    
    return unsubscribe;
  }, [projectId, send, subscribe]);

  const formatFileSize = (bytes: number): string => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  };

  const formatDate = (dateString: string): string => {
    return new Date(dateString).toLocaleDateString('en-US', {
      month: 'short',
      day: 'numeric',
      year: 'numeric'
    });
  };

  const handleDelete = useCallback(async (documentId: string, fileName: string) => {
    if (!confirm(`Delete "${fileName}"? This cannot be undone.`)) return;
    
    setDeleting(documentId);
    
    try {
      await send({
        type: 'document_command',
        method: 'documents.delete',
        params: { document_id: documentId },
      });
      
      // Refresh list after delete
      setTimeout(() => {
        send({
          type: 'document_command',
          method: 'documents.list',
          params: { project_id: projectId },
        });
      }, 100);
    } catch (error) {
      console.error('Delete failed:', error);
    } finally {
      setDeleting(null);
    }
  }, [projectId, send]);

  const handleDownload = useCallback(async (documentId: string, fileName: string) => {
    try {
      await send({
        type: 'document_command',
        method: 'documents.download',
        params: { document_id: documentId },
      });

      const unsubscribe = subscribe('doc-download', (message) => {
        if (message.type === 'data' && message.data?.type === 'document_content') {
          // Decode base64 content
          const content = atob(message.data.content);
          const bytes = new Uint8Array(content.length);
          for (let i = 0; i < content.length; i++) {
            bytes[i] = content.charCodeAt(i);
          }
          
          // Create blob and download
          const blob = new Blob([bytes]);
          const url = URL.createObjectURL(blob);
          const a = document.createElement('a');
          a.href = url;
          a.download = fileName;
          document.body.appendChild(a);
          a.click();
          document.body.removeChild(a);
          URL.revokeObjectURL(url);
          
          unsubscribe();
        }
      });
    } catch (error) {
      console.error('Download failed:', error);
    }
  }, [send, subscribe]);

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12 text-slate-400">
        Loading documents...
      </div>
    );
  }

  if (documents.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center py-12 text-slate-400">
        <FileText className="w-16 h-16 mb-4 text-slate-600" />
        <p>No documents yet</p>
        <p className="text-sm">Upload a document to get started</p>
      </div>
    );
  }

  return (
    <div className="space-y-3">
      {documents.map((doc) => (
        <div
          key={doc.id}
          className="flex items-start justify-between p-4 bg-slate-700/50 rounded-lg border border-slate-600 hover:border-slate-500 transition-colors"
        >
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2 mb-2">
              <FileText className="w-4 h-4 text-blue-400 flex-shrink-0" />
              <h3 className="font-medium text-slate-100 truncate">
                {doc.file_name}
              </h3>
            </div>
            
            <div className="flex items-center gap-3 text-xs text-slate-400">
              <span>{formatFileSize(doc.size_bytes)}</span>
              <span>•</span>
              <span>{doc.word_count?.toLocaleString() || 0} words</span>
              <span>•</span>
              <span>{doc.chunk_count} chunks</span>
              <span>•</span>
              <div className="flex items-center gap-1">
                <Calendar size={12} />
                {formatDate(doc.created_at)}
              </div>
            </div>
          </div>
          
          <div className="flex items-center gap-2 ml-4">
            <button
              onClick={() => handleDownload(doc.id, doc.file_name)}
              className="p-2 text-slate-400 hover:text-blue-400 hover:bg-slate-600 rounded transition-colors"
              title="Download"
            >
              <Download size={16} />
            </button>
            <button
              onClick={() => handleDelete(doc.id, doc.file_name)}
              disabled={deleting === doc.id}
              className="p-2 text-slate-400 hover:text-red-400 hover:bg-slate-600 rounded transition-colors disabled:opacity-50"
              title="Delete"
            >
              <Trash2 size={16} />
            </button>
          </div>
        </div>
      ))}
    </div>
  );
}
