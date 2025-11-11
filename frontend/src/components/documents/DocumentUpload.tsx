// src/components/documents/DocumentUpload.tsx
import { useState, useCallback, useEffect } from 'react';
import { useWebSocketStore } from '../../stores/useWebSocketStore';
import { Upload, CheckCircle, XCircle } from 'lucide-react';

interface DocumentUploadProps {
  projectId: string;
  onUploadComplete?: () => void;
}

export function DocumentUpload({ projectId, onUploadComplete }: DocumentUploadProps) {
  const [uploading, setUploading] = useState(false);
  const [progress, setProgress] = useState(0);
  const [fileName, setFileName] = useState('');
  const [status, setStatus] = useState<'idle' | 'uploading' | 'success' | 'error'>('idle');
  const [errorMessage, setErrorMessage] = useState('');
  const { send, subscribe } = useWebSocketStore();

  useEffect(() => {
    const unsubscribe = subscribe('doc-upload', (message) => {
      // Handle progress updates
      if (message.type === 'data' && message.data?.type === 'document_processing_progress') {
        setProgress(message.data.progress * 100);
        setStatus('uploading');
      }
      
      // Handle completion
      if (message.type === 'data' && message.data?.type === 'document_processed') {
        setStatus('success');
        setProgress(100);
        
        // Reset after 2 seconds
        setTimeout(() => {
          setUploading(false);
          setProgress(0);
          setFileName('');
          setStatus('idle');
          onUploadComplete?.();
        }, 2000);
      }
      
      // Handle errors
      if (message.type === 'error' && uploading) {
        setStatus('error');
        setErrorMessage(message.message || 'Upload failed');
        setUploading(false);
      }
    });
    
    return unsubscribe;
  }, [subscribe, onUploadComplete, uploading]);

  const validateFile = (file: File): string | null => {
    const validExtensions = ['.pdf', '.docx', '.doc', '.txt', '.md'];
    const extension = '.' + file.name.split('.').pop()?.toLowerCase();
    
    if (!validExtensions.includes(extension)) {
      return `Invalid file type. Accepted formats: ${validExtensions.join(', ')}`;
    }
    
    // 50MB max file size
    if (file.size > 50 * 1024 * 1024) {
      return 'File too large. Maximum size is 50MB';
    }
    
    return null;
  };

  const handleFileSelect = useCallback(async (file: File) => {
    // Validate file
    const validationError = validateFile(file);
    if (validationError) {
      setStatus('error');
      setErrorMessage(validationError);
      setTimeout(() => {
        setStatus('idle');
        setErrorMessage('');
      }, 3000);
      return;
    }
    
    setUploading(true);
    setProgress(0);
    setFileName(file.name);
    setStatus('uploading');
    setErrorMessage('');

    const reader = new FileReader();
    
    reader.onerror = () => {
      setStatus('error');
      setErrorMessage('Failed to read file');
      setUploading(false);
    };
    
    reader.onload = async (e) => {
      try {
        const result = e.target?.result?.toString();
        if (!result) {
          throw new Error('Failed to read file content');
        }
        
        // Extract base64 content (remove data:... prefix)
        const base64 = result.split(',')[1];
        
        if (!base64) {
          throw new Error('Invalid file content');
        }

        await send({
          type: 'document_command',
          method: 'documents.upload',
          params: {
            project_id: projectId,
            file_name: file.name,
            content: base64,
          },
        });
      } catch (error) {
        console.error('Upload error:', error);
        setStatus('error');
        setErrorMessage(error instanceof Error ? error.message : 'Upload failed');
        setUploading(false);
      }
    };
    
    reader.readAsDataURL(file);
  }, [projectId, send]);

  const handleDrop = useCallback((e: React.DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    e.stopPropagation();
    
    const files = e.dataTransfer.files;
    if (files.length > 0 && !uploading) {
      handleFileSelect(files[0]);
    }
  }, [uploading, handleFileSelect]);

  const handleDragOver = useCallback((e: React.DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    e.stopPropagation();
  }, []);

  const handleClick = () => {
    if (status === 'idle') {
      document.getElementById('file-upload-input')?.click();
    }
  };

  return (
    <div
      onDrop={handleDrop}
      onDragOver={handleDragOver}
      onClick={handleClick}
      className={`
        p-8 border-2 border-dashed rounded-lg transition-colors cursor-pointer
        ${status === 'success' 
          ? 'border-green-500 bg-green-500/10' 
          : status === 'error'
          ? 'border-red-500 bg-red-500/10'
          : uploading
          ? 'border-blue-500 bg-blue-500/10'
          : 'border-slate-600 hover:border-slate-500 bg-slate-800/50'
        }
      `}
    >
      <input
        type="file"
        id="file-upload-input"
        accept=".pdf,.docx,.doc,.txt,.md"
        onChange={(e) => {
          const file = e.target.files?.[0];
          if (file) handleFileSelect(file);
        }}
        className="hidden"
      />
      
      <div className="flex flex-col items-center">
        {status === 'success' ? (
          <>
            <CheckCircle className="w-12 h-12 text-green-500 mb-3" />
            <h3 className="text-lg font-semibold text-slate-100 mb-1">Upload Complete!</h3>
            <p className="text-sm text-slate-400">{fileName} has been processed and indexed</p>
          </>
        ) : status === 'error' ? (
          <>
            <XCircle className="w-12 h-12 text-red-500 mb-3" />
            <h3 className="text-lg font-semibold text-slate-100 mb-1">Upload Failed</h3>
            <p className="text-sm text-red-400 mb-3">{errorMessage}</p>
            <button
              onClick={(e) => {
                e.stopPropagation();
                setStatus('idle');
                setErrorMessage('');
              }}
              className="px-4 py-2 bg-slate-700 hover:bg-slate-600 rounded text-slate-300 font-medium transition-colors"
            >
              Try Again
            </button>
          </>
        ) : uploading ? (
          <>
            <Upload className="w-12 h-12 text-blue-500 mb-3 animate-pulse" />
            <h3 className="text-lg font-semibold text-slate-100 mb-1">Uploading...</h3>
            <p className="text-sm text-slate-400 mb-3">{fileName}</p>
            <div className="w-full max-w-xs bg-slate-700 rounded-full h-2 overflow-hidden">
              <div 
                className="bg-blue-500 h-full transition-all duration-300"
                style={{ width: `${progress}%` }}
              />
            </div>
            <p className="text-xs text-slate-500 mt-2">{Math.round(progress)}%</p>
          </>
        ) : (
          <>
            <Upload className="w-12 h-12 text-slate-400 mb-3" />
            <h3 className="text-lg font-semibold text-slate-100 mb-1">
              Drop file here or click to browse
            </h3>
            <p className="text-sm text-slate-400">PDF, DOCX, TXT, MD • Max 50MB</p>
          </>
        )}
      </div>
    </div>
  );
}
