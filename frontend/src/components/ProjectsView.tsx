// src/components/ProjectsView.tsx
// REFACTORED: Extracted modals and operations into hooks

import React, { useState, useEffect } from 'react';
import { Plus, Folder, Github, Trash2, Clock, Tag, GitBranch, FileText, Settings } from 'lucide-react';
import { useAppState } from '../stores/useAppState';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { useProjectOperations } from '../hooks/useProjectOperations';
import { useGitOperations } from '../hooks/useGitOperations';
import { CreateProjectModal } from './CreateProjectModal';
import { DeleteConfirmModal } from './DeleteConfirmModal';
import { DocumentsModal } from './documents';
import { CodebaseAttachModal } from './CodebaseAttachModal';
import { ProjectSettingsModal } from './ProjectSettingsModal';
import type { Project } from '../types';

export const ProjectsView: React.FC = () => {
  const { projects, currentProject, setProjects } = useAppState();
  const { subscribe } = useWebSocketStore();

  // Custom hooks for operations
  const {
    createProject,
    deleteProject,
    selectProject,
    refreshProjects,
    creating,
    deleting,
  } = useProjectOperations();

  const { attachCodebase, isAttaching } = useGitOperations();

  // Modal state
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<{ id: string; name: string } | null>(null);
  const [attachTarget, setAttachTarget] = useState<string | null>(null);
  const [showDocuments, setShowDocuments] = useState(false);
  const [settingsTarget, setSettingsTarget] = useState<{ id: string; name: string } | null>(null);

  // Load projects on mount
  useEffect(() => {
    console.log('ProjectsView: Loading projects from backend');
    refreshProjects();

    const unsubscribe = subscribe('projects-initial-load', (message) => {
      if (message.type === 'data' && message.data?.type === 'project_list') {
        console.log('ProjectsView: Received projects:', message.data.projects?.length || 0);
        setProjects(message.data.projects || []);
      }
    });

    return unsubscribe;
  }, [subscribe, setProjects, refreshProjects]);

  // Handle codebase attachment
  const handleAttach = async (type: 'local' | 'git', data: { path?: string; url?: string }) => {
    if (!attachTarget) return;

    const success = await attachCodebase(attachTarget, type, data);
    if (success) {
      setAttachTarget(null);
      // Refresh projects after attachment
      setTimeout(refreshProjects, 100);
    }
  };

  // Handle project deletion
  const handleDeleteConfirm = async () => {
    if (!deleteTarget) return;
    await deleteProject(deleteTarget.id);
  };

  // Utility functions
  const formatDate = (dateString: string) => {
    return new Date(dateString).toLocaleDateString('en-US', {
      month: 'short',
      day: 'numeric',
      year: 'numeric'
    });
  };

  const truncateDescription = (desc: string | undefined, maxLength: number = 60) => {
    if (!desc) return '';
    return desc.length > maxLength ? `${desc.slice(0, maxLength)}...` : desc;
  };

  const getProjectIcon = (project: Project) => {
    if (project.has_repository) {
      return <Github className="text-purple-400" size={20} />;
    } else if (project.has_codebase) {
      return <Folder className="text-blue-400" size={20} />;
    }
    return <Folder className="text-gray-400" size={20} />;
  };

  const getProjectBadge = (project: Project) => {
    if (project.has_repository) {
      return (
        <span className="flex items-center gap-1 text-xs bg-purple-900/30 text-purple-400 px-2 py-0.5 rounded">
          <GitBranch size={12} />
          Repository
        </span>
      );
    } else if (project.has_codebase) {
      return (
        <span className="flex items-center gap-1 text-xs bg-blue-900/30 text-blue-400 px-2 py-0.5 rounded">
          <Folder size={12} />
          Codebase
        </span>
      );
    }
    return null;
  };

  return (
    <div className="h-full flex flex-col bg-gray-50 dark:bg-slate-900">
      {/* Header */}
      <div className="flex-shrink-0 px-4 py-3 border-b border-gray-200 dark:border-slate-700">
        <div className="flex items-center justify-between">
          <div>
            <h2 className="text-lg font-semibold text-gray-800 dark:text-slate-200">Projects</h2>
            <p className="text-sm text-gray-500 dark:text-slate-400">{projects.length} total</p>
          </div>
          <div className="flex gap-2">
            {currentProject && (
              <button
                onClick={() => setShowDocuments(true)}
                className="flex items-center gap-2 px-3 py-1.5 bg-gray-200 dark:bg-slate-700 hover:bg-gray-300 dark:hover:bg-slate-600 text-gray-700 dark:text-slate-200 rounded transition-colors text-sm"
              >
                <FileText size={16} />
                Documents
              </button>
            )}
            <button
              onClick={() => setShowCreateModal(true)}
              className="flex items-center gap-2 px-3 py-1.5 bg-blue-600 hover:bg-blue-700 text-white rounded transition-colors text-sm"
            >
              <Plus size={16} />
              New Project
            </button>
          </div>
        </div>
      </div>

      {/* Project List */}
      <div className="flex-1 overflow-y-auto p-4">
        {projects.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full text-center py-12">
            <Folder size={64} className="text-gray-400 dark:text-slate-600 mb-4" />
            <h3 className="text-lg font-medium text-gray-500 dark:text-slate-400 mb-2">No Projects Yet</h3>
            <p className="text-sm text-gray-400 dark:text-slate-500 mb-4 max-w-md">
              Create your first project to get started with Mira
            </p>
            <button
              onClick={() => setShowCreateModal(true)}
              className="flex items-center gap-2 px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded transition-colors"
            >
              <Plus size={18} />
              Create Project
            </button>
          </div>
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
            {projects.map((project) => (
              <div
                key={project.id}
                className={`
                  p-4 rounded-lg border-2 transition-all cursor-pointer
                  ${currentProject?.id === project.id
                    ? 'border-blue-500 bg-blue-50 dark:bg-blue-900/20'
                    : 'border-gray-200 dark:border-slate-700 bg-white dark:bg-slate-800 hover:border-gray-300 dark:hover:border-slate-600'
                  }
                `}
                onClick={() => selectProject(project)}
              >
                {/* Project Header */}
                <div className="flex items-start gap-3 mb-3">
                  <div className="mt-0.5">
                    {getProjectIcon(project)}
                  </div>
                  <div className="flex-1 min-w-0">
                    <h3 className="text-base font-semibold text-gray-800 dark:text-slate-200 truncate">
                      {project.name}
                    </h3>
                    {project.description && (
                      <p className="text-sm text-gray-500 dark:text-slate-400 mt-1">
                        {truncateDescription(project.description)}
                      </p>
                    )}
                  </div>
                </div>

                {/* Project Metadata */}
                <div className="flex flex-wrap items-center gap-2 mb-3">
                  {getProjectBadge(project)}
                  {project.tags && project.tags.length > 0 && (
                    <span className="flex items-center gap-1 text-xs bg-gray-200 dark:bg-slate-700 text-gray-600 dark:text-slate-300 px-2 py-0.5 rounded">
                      <Tag size={12} />
                      {project.tags[0]}
                      {project.tags.length > 1 && ` +${project.tags.length - 1}`}
                    </span>
                  )}
                </div>

                <div className="flex items-center gap-1 text-xs text-gray-400 dark:text-slate-500 mb-3">
                  <Clock size={12} />
                  {formatDate(project.updated_at || project.created_at)}
                </div>

                {/* Actions */}
                <div className="flex gap-2 pt-2 border-t border-gray-200 dark:border-slate-700">
                  {!project.has_codebase && !project.has_repository && (
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        setAttachTarget(project.id);
                      }}
                      className="flex-1 px-2 py-1.5 text-xs bg-gray-200 dark:bg-slate-700 hover:bg-gray-300 dark:hover:bg-slate-600 text-gray-700 dark:text-slate-200 rounded transition-colors"
                    >
                      Attach Codebase
                    </button>
                  )}
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      setSettingsTarget({ id: project.id, name: project.name });
                    }}
                    className="px-2 py-1.5 text-xs bg-gray-200 dark:bg-slate-700 hover:bg-gray-300 dark:hover:bg-slate-600 text-gray-700 dark:text-slate-200 rounded transition-colors flex items-center gap-1"
                    title="Project settings"
                  >
                    <Settings size={12} />
                    Settings
                  </button>
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      setDeleteTarget({ id: project.id, name: project.name });
                    }}
                    className="px-2 py-1.5 text-xs bg-red-100 dark:bg-red-900/30 hover:bg-red-200 dark:hover:bg-red-900/50 text-red-600 dark:text-red-400 rounded transition-colors flex items-center gap-1"
                    disabled={deleting === project.id}
                  >
                    <Trash2 size={12} />
                    {deleting === project.id ? 'Deleting...' : 'Delete'}
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Modals */}
      <CreateProjectModal
        isOpen={showCreateModal}
        onClose={() => setShowCreateModal(false)}
        onCreate={createProject}
        creating={creating}
      />

      <DeleteConfirmModal
        isOpen={deleteTarget !== null}
        projectName={deleteTarget?.name || ''}
        onClose={() => setDeleteTarget(null)}
        onConfirm={handleDeleteConfirm}
        deleting={deleting !== null}
      />

      {attachTarget && (
        <CodebaseAttachModal
          projectId={attachTarget}
          onClose={() => setAttachTarget(null)}
          onAttach={handleAttach}
        />
      )}

      {showDocuments && currentProject && (
        <DocumentsModal
          projectId={currentProject.id}
          projectName={currentProject.name}
          onClose={() => setShowDocuments(false)}
        />
      )}

      {settingsTarget && (
        <ProjectSettingsModal
          projectId={settingsTarget.id}
          projectName={settingsTarget.name}
          isOpen={true}
          onClose={() => setSettingsTarget(null)}
        />
      )}
    </div>
  );
};
