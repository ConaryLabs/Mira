// src/components/documents/DocumentsView.tsx
import { useState } from 'react';
import { DocumentUpload } from './DocumentUpload';
import { DocumentList } from './DocumentList';
import { DocumentSearch } from './DocumentSearch';
import { Upload as UploadIcon, FileText, Search } from 'lucide-react';

interface DocumentsViewProps {
  projectId: string;
}

export function DocumentsView({ projectId }: DocumentsViewProps) {
  const [refreshKey, setRefreshKey] = useState(0);

  const handleUploadComplete = () => {
    // Force list refresh by changing key
    setRefreshKey(prev => prev + 1);
  };

  return (
    <div className="h-full flex flex-col bg-slate-900 text-slate-200">
      {/* Main Content - Two Column Layout */}
      <div className="h-full grid grid-cols-2 gap-6 p-6">
        {/* Left Column: Upload & List */}
        <div className="space-y-6 overflow-y-auto">
          {/* Upload Section */}
          <div>
            <div className="flex items-center gap-2 mb-4">
              <UploadIcon className="w-5 h-5 text-blue-500" />
              <h3 className="text-lg font-semibold text-slate-200">Upload Documents</h3>
            </div>
            <DocumentUpload 
              projectId={projectId} 
              onUploadComplete={handleUploadComplete}
            />
          </div>

          {/* Document List Section */}
          <div>
            <div className="flex items-center gap-2 mb-4">
              <FileText className="w-5 h-5 text-blue-500" />
              <h3 className="text-lg font-semibold text-slate-200">Your Documents</h3>
            </div>
            <DocumentList key={refreshKey} projectId={projectId} />
          </div>
        </div>

        {/* Right Column: Search */}
        <div className="overflow-y-auto">
          <div className="flex items-center gap-2 mb-4">
            <Search className="w-5 h-5 text-blue-500" />
            <h3 className="text-lg font-semibold text-slate-200">Search Documents</h3>
          </div>
          <DocumentSearch projectId={projectId} />
        </div>
      </div>
    </div>
  );
}
