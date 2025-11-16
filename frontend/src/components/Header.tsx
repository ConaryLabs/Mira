// src/components/Header.tsx
import React, { useState } from 'react';
import { Play, Folder, Terminal, X } from 'lucide-react';
import ArtifactToggle from './ArtifactToggle';
import { CommitPushButton } from './CommitPushButton';
import { GitSyncButton } from './GitSyncButton';
import { ProjectsView } from './ProjectsView';
import { useAppState, useArtifactState } from '../stores/useAppState';
import { useTerminalStore } from '../stores/useTerminalStore';

export const Header: React.FC = () => {
  const {
    currentProject,
    showArtifacts,
    setShowArtifacts
  } = useAppState();

  const { artifacts } = useArtifactState();
  const { toggleTerminalVisibility, isTerminalVisible } = useTerminalStore();
  const [showProjects, setShowProjects] = useState(false);

  const handleTerminalClick = () => {
    toggleTerminalVisibility();
  };
  
  return (
    <>
      <header className="h-14 border-b border-gray-700 px-4 flex items-center bg-gray-900">
        {/* Left: Project indicator - clickable */}
        <div className="flex items-center gap-4">
          <button
            onClick={() => setShowProjects(true)}
            className="flex items-center gap-2 px-3 py-1.5 bg-slate-800 hover:bg-slate-700 rounded-lg border border-slate-600 transition-colors"
            title="Manage Projects"
          >
            <Folder size={16} className="text-slate-400" />
            <span className="text-sm text-slate-200">
              {currentProject?.name || 'No Project'}
            </span>
          </button>
        </div>

        <div className="flex-1" />

      {/* Right: Action buttons - only show if project exists */}
      <div className="flex items-center gap-2 ml-auto">
        {currentProject && (
          <>
            <button
              className="p-2 text-gray-400 hover:text-gray-200 hover:bg-gray-800 rounded-md"
              title="Run project"
            >
              <Play size={16} />
            </button>
            
            {/* Git sync button - only show if project has repo */}
            {currentProject.has_repository && (
              <GitSyncButton />
            )}
            
            <CommitPushButton />

            {/* Terminal Toggle */}
            <button
              type="button"
              onClick={handleTerminalClick}
              className={`p-2 rounded-md transition-colors ${
                isTerminalVisible
                  ? 'text-blue-400 bg-blue-900/30'
                  : 'text-gray-400 hover:text-gray-200 hover:bg-gray-800'
              }`}
              title="Toggle Terminal (Ctrl+`)"
            >
              <Terminal size={16} />
            </button>
          </>
        )}

        {/* Artifact Toggle - show when there are artifacts OR project selected */}
        {(artifacts.length > 0 || currentProject) && (
          <ArtifactToggle
            isOpen={showArtifacts}
            onClick={() => setShowArtifacts(!showArtifacts)}
            artifactCount={artifacts.length}
            hasGitRepos={currentProject?.has_repository || false}
            isDark={true}
          />
        )}
      </div>
    </header>

      {/* Projects Modal */}
      {showProjects && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div className="bg-slate-900 border border-slate-700 rounded-lg shadow-2xl w-full max-w-6xl h-[80vh] flex flex-col">
            {/* Modal Header */}
            <div className="flex items-center justify-between px-4 py-3 border-b border-slate-700">
              <h2 className="text-lg font-semibold text-slate-200">Projects</h2>
              <button
                onClick={() => setShowProjects(false)}
                className="p-1.5 text-slate-400 hover:text-slate-200 hover:bg-slate-700 rounded transition-colors"
                title="Close"
              >
                <X size={18} />
              </button>
            </div>

            {/* Modal Content */}
            <div className="flex-1 overflow-hidden">
              <ProjectsView />
            </div>
          </div>
        </div>
      )}
    </>
  );
};
