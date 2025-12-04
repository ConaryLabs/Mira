// src/components/Header.tsx
import React, { useState } from 'react';
import { Folder, Activity, X, LogOut, Brain, Sun, Moon } from 'lucide-react';
import { useNavigate } from 'react-router-dom';
import ArtifactToggle from './ArtifactToggle';
import { ProjectsView } from './ProjectsView';
import { ChangePasswordModal } from './ChangePasswordModal';
import { useAppState, useArtifactState } from '../stores/useAppState';
import { useActivityStore } from '../stores/useActivityStore';
import { useCodeIntelligenceStore } from '../stores/useCodeIntelligenceStore';
import { useAuthStore, useCurrentUser } from '../stores/useAuthStore';
import { useThemeStore } from '../stores/useThemeStore';

export const Header: React.FC = () => {
  const {
    currentProject,
    showArtifacts,
    setShowArtifacts
  } = useAppState();

  const { artifacts } = useArtifactState();
  const { togglePanel, isPanelVisible } = useActivityStore();
  const {
    togglePanel: toggleIntelligence,
    isPanelVisible: isIntelligenceVisible
  } = useCodeIntelligenceStore();
  const [showProjects, setShowProjects] = useState(false);
  const [showChangePassword, setShowChangePassword] = useState(false);
  const { logout } = useAuthStore();
  const user = useCurrentUser();
  const navigate = useNavigate();
  const { theme, toggleTheme } = useThemeStore();

  const handleActivityClick = () => {
    togglePanel();
  };

  const handleIntelligenceClick = () => {
    toggleIntelligence();
  };

  const handleLogout = () => {
    logout();
    navigate('/login');
  };
  
  return (
    <>
      <header className="h-14 border-b border-gray-200 dark:border-gray-700 px-4 flex items-center bg-white dark:bg-gray-900">
        {/* Left: Project indicator - clickable */}
        <div className="flex items-center gap-4">
          <button
            onClick={() => setShowProjects(true)}
            className="flex items-center gap-2 px-3 py-1.5 bg-gray-100 dark:bg-slate-800 hover:bg-gray-200 dark:hover:bg-slate-700 rounded-lg border border-gray-300 dark:border-slate-600 transition-colors"
            title="Manage Projects"
          >
            <Folder size={16} className="text-gray-500 dark:text-slate-400" />
            <span className="text-sm text-gray-700 dark:text-slate-200">
              {currentProject?.name || 'No Project'}
            </span>
          </button>
        </div>

        <div className="flex-1" />

      {/* Right: Action buttons */}
      <div className="flex items-center gap-2 ml-auto">
        {/* User info - clickable to change password */}
        {user && (
          <button
            onClick={() => setShowChangePassword(true)}
            className="px-3 py-1 text-sm text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-200 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-md transition-colors"
            title="Click to change password"
          >
            {user.displayName || user.username}
          </button>
        )}

        {currentProject && (
          <>
            {/* Intelligence Panel Toggle */}
            <button
              type="button"
              onClick={handleIntelligenceClick}
              className={`p-2 rounded-md transition-colors ${
                isIntelligenceVisible
                  ? 'text-purple-600 dark:text-purple-400 bg-purple-100 dark:bg-purple-900/30'
                  : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-200 hover:bg-gray-100 dark:hover:bg-gray-800'
              }`}
              title="Toggle Intelligence Panel (Budget, Search, Co-Change)"
            >
              <Brain size={16} />
            </button>

            {/* Activity Panel Toggle */}
            <button
              type="button"
              onClick={handleActivityClick}
              className={`p-2 rounded-md transition-colors ${
                isPanelVisible
                  ? 'text-blue-600 dark:text-blue-400 bg-blue-100 dark:bg-blue-900/30'
                  : 'text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-200 hover:bg-gray-100 dark:hover:bg-gray-800'
              }`}
              title="Toggle Activity Panel"
            >
              <Activity size={16} />
            </button>
          </>
        )}

        {/* Artifact Toggle - show when there are artifacts OR project selected */}
        {(artifacts.length > 0 || currentProject) && (
          <ArtifactToggle
            isOpen={showArtifacts}
            onClick={() => setShowArtifacts(!showArtifacts)}
            artifactCount={artifacts.length}
            isDark={theme === 'dark'}
          />
        )}

        {/* Theme toggle */}
        <button
          type="button"
          onClick={toggleTheme}
          className="p-2 text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-200 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-md transition-colors"
          title={theme === 'light' ? 'Switch to dark mode' : 'Switch to light mode'}
        >
          {theme === 'light' ? <Moon size={16} /> : <Sun size={16} />}
        </button>

        {/* Logout button */}
        <button
          type="button"
          onClick={handleLogout}
          className="p-2 text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-200 hover:bg-gray-100 dark:hover:bg-gray-800 rounded-md transition-colors"
          title="Logout"
        >
          <LogOut size={16} />
        </button>
      </div>
    </header>

      {/* Projects Modal */}
      {showProjects && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div className="bg-white dark:bg-slate-900 border border-gray-200 dark:border-slate-700 rounded-lg shadow-2xl w-full max-w-6xl h-[80vh] flex flex-col">
            {/* Modal Header */}
            <div className="flex items-center justify-between px-4 py-3 border-b border-gray-200 dark:border-slate-700">
              <h2 className="text-lg font-semibold text-gray-900 dark:text-slate-200">Projects</h2>
              <button
                onClick={() => setShowProjects(false)}
                className="p-1.5 text-gray-500 dark:text-slate-400 hover:text-gray-700 dark:hover:text-slate-200 hover:bg-gray-100 dark:hover:bg-slate-700 rounded transition-colors"
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

      {/* Change Password Modal */}
      <ChangePasswordModal
        isOpen={showChangePassword}
        onClose={() => setShowChangePassword(false)}
      />
    </>
  );
};
