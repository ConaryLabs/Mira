// src/components/ArtifactToggle.tsx
import React from 'react';
import { FileText, GitBranch } from 'lucide-react';

interface ArtifactToggleProps {
  isOpen: boolean;
  onClick: () => void;
  artifactCount: number;
  hasGitRepos?: boolean;
  isDark: boolean;
}

const ArtifactToggle: React.FC<ArtifactToggleProps> = ({
  isOpen,
  onClick,
  artifactCount,
  hasGitRepos = true, // Default to true if project is selected
  isDark,
}) => {
  return (
    <button
      onClick={onClick}
      className={`
        relative p-3 rounded-full shadow-lg transition-all duration-200 transform
        ${isOpen ? 'scale-95' : 'hover:scale-105'}
        ${isDark 
          ? 'bg-gray-800 hover:bg-gray-700 text-gray-200' 
          : 'bg-white hover:bg-gray-50 text-gray-700'
        }
        border-2 ${isDark ? 'border-gray-700' : 'border-gray-200'}
      `}
      title={isOpen ? 'Close resources' : 'Open project resources'}
    >
      {hasGitRepos ? (
        <GitBranch className="w-6 h-6" />
      ) : (
        <FileText className="w-6 h-6" />
      )}
      
      {(artifactCount > 0 || hasGitRepos) && (
        <span className={`
          absolute -top-1 -right-1 min-w-[20px] h-5 px-1 
          flex items-center justify-center rounded-full text-xs font-bold
          ${isDark 
            ? 'bg-blue-600 text-white' 
            : 'bg-blue-500 text-white'
          }
        `}>
          {artifactCount > 0 ? artifactCount : <GitBranch className="w-3 h-3" />}
        </span>
      )}
    </button>
  );
};

export default ArtifactToggle;
