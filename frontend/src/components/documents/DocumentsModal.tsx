// src/components/documents/DocumentsModal.tsx
import { X, FileText } from 'lucide-react';
import { DocumentsView } from './DocumentsView';

interface DocumentsModalProps {
  projectId: string;
  projectName: string;
  onClose: () => void;
}

export function DocumentsModal({ projectId, projectName, onClose }: DocumentsModalProps) {
  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4">
      <div className="bg-slate-800 rounded-lg border border-slate-700 w-full max-w-7xl h-[90vh] flex flex-col">
        {/* Modal Header */}
        <div className="flex items-center justify-between p-6 border-b border-slate-700">
          <div className="flex items-center gap-3">
            <FileText className="w-6 h-6 text-blue-500" />
            <div>
              <h2 className="text-xl font-bold text-slate-100">
                Documents - {projectName}
              </h2>
              <p className="text-sm text-slate-400">
                Upload, search, and manage project documents
              </p>
            </div>
          </div>
          <button
            onClick={onClose}
            className="p-2 text-slate-400 hover:text-slate-100 hover:bg-slate-700 rounded transition-colors"
          >
            <X size={20} />
          </button>
        </div>

        {/* Documents View Content */}
        <div className="flex-1 overflow-hidden">
          <DocumentsView projectId={projectId} />
        </div>

        {/* Footer Info */}
        <div className="px-6 py-3 border-t border-slate-700 bg-slate-850">
          <div className="flex items-center justify-between text-xs text-slate-500">
            <div className="flex items-center gap-4">
              <span>Supported formats: PDF, DOCX, TXT, MD</span>
              <span>•</span>
              <span>Max file size: 50MB</span>
            </div>
            <div className="flex items-center gap-2">
              <div className="w-2 h-2 bg-green-500 rounded-full animate-pulse"></div>
              <span>Semantic search enabled</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
