// src/components/ToastContainer.tsx
// Displays toast notifications in the bottom-right corner

import React from 'react';
import { useAppState } from '../stores/useAppState';
import { X, AlertCircle, CheckCircle, Info, AlertTriangle } from 'lucide-react';

export const ToastContainer: React.FC = () => {
  const toasts = useAppState(state => state.toasts);
  const removeToast = useAppState(state => state.removeToast);
  
  if (toasts.length === 0) return null;
  
  const getIcon = (type: string) => {
    switch (type) {
      case 'success': return <CheckCircle className="w-5 h-5" />;
      case 'error': return <AlertCircle className="w-5 h-5" />;
      case 'warning': return <AlertTriangle className="w-5 h-5" />;
      case 'info': return <Info className="w-5 h-5" />;
      default: return <Info className="w-5 h-5" />;
    }
  };
  
  const getColorClasses = (type: string) => {
    switch (type) {
      case 'success': 
        return 'bg-green-900 border-green-700 text-green-100';
      case 'error': 
        return 'bg-red-900 border-red-700 text-red-100';
      case 'warning': 
        return 'bg-yellow-900 border-yellow-700 text-yellow-100';
      case 'info': 
        return 'bg-blue-900 border-blue-700 text-blue-100';
      default: 
        return 'bg-gray-900 border-gray-700 text-gray-100';
    }
  };
  
  return (
    <div className="fixed bottom-4 right-4 z-50 space-y-2 pointer-events-none">
      {toasts.map(toast => (
        <div
          key={toast.id}
          className={`
            pointer-events-auto
            flex items-start gap-3 p-4 rounded-lg border
            shadow-lg backdrop-blur-sm
            animate-slide-in-right
            max-w-md
            ${getColorClasses(toast.type)}
          `}
        >
          <div className="flex-shrink-0 mt-0.5">
            {getIcon(toast.type)}
          </div>
          
          <div className="flex-1 min-w-0">
            <p className="text-sm font-medium break-words">
              {toast.message}
            </p>
          </div>
          
          <button
            onClick={() => removeToast(toast.id)}
            className="flex-shrink-0 ml-2 opacity-70 hover:opacity-100 transition-opacity"
            aria-label="Dismiss"
          >
            <X className="w-4 h-4" />
          </button>
        </div>
      ))}
    </div>
  );
};

// Add this to your global CSS or Tailwind config for the animation:
/*
@keyframes slide-in-right {
  from {
    transform: translateX(100%);
    opacity: 0;
  }
  to {
    transform: translateX(0);
    opacity: 1;
  }
}

.animate-slide-in-right {
  animation: slide-in-right 0.3s ease-out;
}
*/
