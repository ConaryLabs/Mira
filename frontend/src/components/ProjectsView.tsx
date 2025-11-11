// src/components/ProjectsView.tsx
import React, { useState, useEffect } from 'react';
import { 
  Plus, Folder, Github, Trash2, 
  Clock, Tag, GitBranch, X, Check, FileText
} from 'lucide-react';
import { useAppState } from '../stores/useAppState';
import { useWebSocketStore } from '../stores/useWebSocketStore';
import { DocumentsModal } from './documents';
import { CodebaseAttachModal } from './CodebaseAttachModal';
import type { Project } from '../types';

export const ProjectsView: React.FC = () => {
  const { projects, currentProject, setCurrentProject, setProjects } = useAppState();
  const { send, subscribe } = useWebSocketStore();
  
  // Project state
  const [showCreateForm, setShowCreateForm] = useState(false);
  const [newProjectName, setNewProjectName] = useState('');
  const [newProjectDescription, setNewProjectDescription] = useState('');
  const [creating, setCreating] = useState(false);
  const [deleting, setDeleting] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  
  // Codebase attachment state
  const [showAttachCodebase, setShowAttachCodebase] = useState<string | null>(null);
  
  // Documents modal state
  const [showDocuments, setShowDocuments] = useState(false);
  
  // ===== LOAD PROJECTS ON MOUNT =====
  useEffect(() => {
    console.log('ProjectsView: Loading projects from backend');
    
    send({
      type: 'project_command',
      method: 'project.list',
      params: {}
    });

    const unsubscribe = subscribe('projects-initial-load', (message) => {
      if (message.type === 'data' && message.data?.type === 'project_list') {
        console.log('ProjectsView: Received projects from backend:', message.data.projects?.length || 0);
        setProjects(message.data.projects || []);
      }
    });

    return unsubscribe;
  }, [send, subscribe, setProjects]);
  
  // ===== PROJECT HANDLERS =====
  
  const handleCreate = async () => {
    if (!newProjectName.trim()) return;
    
    setCreating(true);
    try {
      await send({
        type: 'project_command',
        method: 'project.create',
        params: { 
          name: newProjectName.trim(),
          description: newProjectDescription.trim() || undefined
        }
      });
      
      setNewProjectName('');
      setNewProjectDescription('');
      setShowCreateForm(false);
      
      // Refresh project list
      setTimeout(() => {
        send({
          type: 'project_command',
          method: 'project.list',
          params: {}
        });
      }, 100);
    } catch (error) {
      console.error('Create failed:', error);
    } finally {
      setCreating(false);
    }
  };
  
  const handleDelete = async (projectId: string) => {
    setDeleting(projectId);
    
    try {
      await send({
        type: 'project_command',
        method: 'project.delete',
        params: { id: projectId }  // ✅ Backend expects "id", not "project_id"
      });
      
      if (currentProject?.id === projectId) {
        setCurrentProject(null);
      }
      
      // Refresh project list
      setTimeout(() => {
        send({
          type: 'project_command',
          method: 'project.list',
          params: {}
        });
      }, 100);
    } catch (error) {
      console.error('Delete failed:', error);
    } finally {
      setDeleting(null);
      setConfirmDelete(null);
    }
  };
  
  const handleSelectProject = (project: Project) => {
    setCurrentProject(project);
  };
  
  const handleAttachCodebase = async (
    type: 'local' | 'git', 
    data: { path?: string; url?: string }
  ) => {
    if (!showAttachCodebase) return;
    
    try {
      if (type === 'local') {
        await send({
          type: 'project_command',
          method: 'project.attach_local',
          params: {
            project_id: showAttachCodebase,
            directory_path: data.path
          }
        });
      } else {
        // Git import is 3 steps: attach → clone → import
        console.log('Step 1: Attaching repository...');
        await send({
          type: 'git_command',
          method: 'git.attach',
          params: {
            project_id: showAttachCodebase,
            repo_url: data.url
          }
        });
        
        // Wait for attach to complete
        await new Promise(resolve => setTimeout(resolve, 1000));
        
        console.log('Step 2: Cloning repository...');
        await send({
          type: 'git_command',
          method: 'git.clone',
          params: {
            project_id: showAttachCodebase
          }
        });
        
        // Wait for clone to complete (this can take a while)
        await new Promise(resolve => setTimeout(resolve, 5000));
        
        console.log('Step 3: Importing codebase...');
        await send({
          type: 'git_command',
          method: 'git.import',
          params: {
            project_id: showAttachCodebase
          }
        });
        
        // Wait for import to complete
        await new Promise(resolve => setTimeout(resolve, 2000));
      }
      
      // Refresh project list
      setTimeout(() => {
        send({
          type: 'project_command',
          method: 'project.list',
          params: {}
        });
      }, 100);
    } catch (error) {
      console.error('Attach failed:', error);
      throw error;
    }
  };
  
  // ===== UTILITY FUNCTIONS =====
  
  const formatDate = (dateString: string) => {
    return new Date(dateString).toLocaleDateString('en-US', {
      month: 'short',
      day: 'numeric',
      year: 'numeric'
    });
  };
  
  // ===== RENDER =====
  
  return (
    <div className="flex-1 flex flex-col bg-slate-900 text-slate-100">
      {/* Header */}
      <div className="flex items-center justify-between px-6 py-4 border-b border-slate-700">
        <div className="flex items-center gap-3">
          <Folder className="w-6 h-6 text-blue-500" />
          <h1 className="text-xl font-bold">Projects</h1>
          <span className="text-sm text-slate-400">({projects.length})</span>
        </div>
        
        <button
          onClick={() => setShowCreateForm(true)}
          className="flex items-center gap-2 px-4 py-2 bg-blue-600 hover:bg-blue-700 rounded-lg transition-colors font-medium"
        >
          <Plus size={18} />
          New Project
        </button>
      </div>
      
      {/* Create Project Modal */}
      {showCreateForm && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div className="bg-slate-800 rounded-lg border border-slate-700 p-6 w-full max-w-md">
            <h2 className="text-xl font-bold text-slate-100 mb-4">Create New Project</h2>
            
            <div className="space-y-4">
              <div>
                <label className="block text-sm font-medium text-slate-300 mb-2">
                  Project Name *
                </label>
                <input
                  type="text"
                  value={newProjectName}
                  onChange={(e) => setNewProjectName(e.target.value)}
                  placeholder="My Awesome Project"
                  className="w-full px-3 py-2 bg-slate-900 border border-slate-600 rounded-lg text-slate-100 placeholder-slate-500 focus:outline-none focus:border-blue-500"
                  autoFocus
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' && !e.shiftKey && newProjectName.trim()) {
                      e.preventDefault();
                      handleCreate();
                    }
                    if (e.key === 'Escape') {
                      setShowCreateForm(false);
                      setNewProjectName('');
                      setNewProjectDescription('');
                    }
                  }}
                />
              </div>
              
              <div>
                <label className="block text-sm font-medium text-slate-300 mb-2">
                  Description
                </label>
                <textarea
                  value={newProjectDescription}
                  onChange={(e) => setNewProjectDescription(e.target.value)}
                  placeholder="What's this project about?"
                  className="w-full px-3 py-2 bg-slate-900 border border-slate-600 rounded-lg text-slate-100 placeholder-slate-500 focus:outline-none focus:border-blue-500 resize-none"
                  rows={3}
                />
              </div>
              
              <div className="flex gap-3">
                <button
                  onClick={handleCreate}
                  disabled={!newProjectName.trim() || creating}
                  className="flex-1 px-4 py-2 bg-blue-600 hover:bg-blue-700 disabled:bg-slate-700 disabled:text-slate-500 rounded-lg text-white font-medium transition-colors"
                >
                  {creating ? 'Creating...' : 'Create Project'}
                </button>
                <button
                  onClick={() => {
                    setShowCreateForm(false);
                    setNewProjectName('');
                    setNewProjectDescription('');
                  }}
                  className="px-4 py-2 bg-slate-700 hover:bg-slate-600 rounded-lg text-slate-300 font-medium transition-colors"
                >
                  Cancel
                </button>
              </div>
            </div>
          </div>
        </div>
      )}
      
      {/* Confirm Delete Modal */}
      {confirmDelete && (
        <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
          <div className="bg-slate-800 rounded-lg border border-slate-700 p-6 w-full max-w-md">
            <h2 className="text-xl font-bold text-slate-100 mb-4">Delete Project?</h2>
            <p className="text-slate-300 mb-6">
              This will permanently delete the project and all its data. This action cannot be undone.
            </p>
            
            <div className="flex gap-3">
              <button
                onClick={() => handleDelete(confirmDelete)}
                disabled={deleting === confirmDelete}
                className="flex-1 px-4 py-2 bg-red-600 hover:bg-red-700 disabled:bg-slate-700 disabled:text-slate-500 rounded-lg text-white font-medium transition-colors"
              >
                {deleting === confirmDelete ? 'Deleting...' : 'Delete Forever'}
              </button>
              <button
                onClick={() => setConfirmDelete(null)}
                className="px-4 py-2 bg-slate-700 hover:bg-slate-600 rounded-lg text-slate-300 font-medium transition-colors"
              >
                Cancel
              </button>
            </div>
          </div>
        </div>
      )}
      
      {/* Attach Codebase Modal */}
      {showAttachCodebase && (
        <CodebaseAttachModal
          projectId={showAttachCodebase}
          onClose={() => setShowAttachCodebase(null)}
          onAttach={handleAttachCodebase}
        />
      )}
      
      {/* Documents Modal */}
      {showDocuments && currentProject && (
        <DocumentsModal
          projectId={currentProject.id}
          projectName={currentProject.name}
          onClose={() => setShowDocuments(false)}
        />
      )}
      
      {/* Project Grid */}
      <div className="flex-1 overflow-y-auto p-6">
        {projects.length === 0 ? (
          <div className="flex flex-col items-center justify-center h-full text-slate-400">
            <Folder size={64} className="mb-4 text-slate-600" />
            <h3 className="text-xl font-medium mb-2">No projects yet</h3>
            <p className="text-sm mb-6">Create your first project to get started</p>
            <button
              onClick={() => setShowCreateForm(true)}
              className="flex items-center gap-2 px-4 py-2 bg-blue-600 hover:bg-blue-700 rounded-lg text-white font-medium transition-colors"
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
                onClick={() => handleSelectProject(project)}
                className={`
                  relative p-6 rounded-lg border-2 transition-all cursor-pointer
                  ${currentProject?.id === project.id
                    ? 'border-blue-500 bg-slate-800'
                    : 'border-slate-700 bg-slate-800/50 hover:border-slate-600'
                  }
                `}
              >
                {/* Active indicator */}
                {currentProject?.id === project.id && (
                  <div className="absolute top-4 right-4">
                    <div className="flex items-center gap-1 px-2 py-1 bg-blue-600 rounded text-xs font-medium text-white">
                      <Check size={12} />
                      Active
                    </div>
                  </div>
                )}
                
                {/* Project header */}
                <div className="flex items-start gap-3 mb-4">
                  <div className={`
                    p-2 rounded-lg
                    ${currentProject?.id === project.id ? 'bg-blue-600' : 'bg-slate-700'}
                  `}>
                    <Folder size={20} className="text-slate-100" />
                  </div>
                  <div className="flex-1 min-w-0">
                    <h3 className="text-lg font-semibold text-slate-100 truncate">
                      {project.name}
                    </h3>
                    {project.has_repository && (
                      <div className="flex items-center gap-1 mt-1 text-xs text-green-400">
                        <GitBranch size={12} />
                        Repository attached
                      </div>
                    )}
                  </div>
                </div>
                
                {/* Description */}
                {project.description && (
                  <p className="text-sm text-slate-400 mb-4 line-clamp-2">
                    {project.description}
                  </p>
                )}
                
                {/* Metadata */}
                <div className="flex items-center gap-4 text-xs text-slate-500 mb-4">
                  <div className="flex items-center gap-1">
                    <Clock size={12} />
                    {formatDate(project.created_at)}
                  </div>
                  {project.tags && project.tags.length > 0 && (
                    <div className="flex items-center gap-1">
                      <Tag size={12} />
                      {project.tags.length}
                    </div>
                  )}
                </div>
                
                {/* Tags */}
                {project.tags && project.tags.length > 0 && (
                  <div className="flex flex-wrap gap-2 mb-4">
                    {project.tags.slice(0, 3).map((tag, idx) => (
                      <span
                        key={idx}
                        className="px-2 py-1 bg-slate-700 text-slate-300 text-xs rounded"
                      >
                        {tag}
                      </span>
                    ))}
                    {project.tags.length > 3 && (
                      <span className="px-2 py-1 text-slate-500 text-xs">
                        +{project.tags.length - 3} more
                      </span>
                    )}
                  </div>
                )}
                
                {/* Actions (only show for active project) */}
                {currentProject?.id === project.id && (
                  <div className="space-y-2">
                    {!project.has_repository && (
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          setShowAttachCodebase(project.id);
                        }}
                        className="w-full flex items-center justify-center gap-2 px-3 py-2 bg-slate-700 hover:bg-slate-600 rounded transition-colors text-sm"
                      >
                        <Folder size={14} />
                        Attach Codebase
                      </button>
                    )}
                    
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        setShowDocuments(true);
                      }}
                      className="w-full flex items-center justify-center gap-2 px-3 py-2 bg-blue-600 hover:bg-blue-700 rounded transition-colors text-sm text-white font-medium"
                    >
                      <FileText size={14} />
                      Documents
                    </button>
                  </div>
                )}
                
                {/* Delete button */}
                <button
                  onClick={(e) => {
                    e.stopPropagation();
                    setConfirmDelete(project.id);
                  }}
                  className="absolute bottom-4 right-4 p-2 text-slate-400 hover:text-red-400 hover:bg-slate-700 rounded transition-colors"
                  title="Delete project"
                >
                  <Trash2 size={16} />
                </button>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
};
