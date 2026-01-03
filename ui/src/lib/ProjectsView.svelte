<script lang="ts">
  import { onMount } from 'svelte';
  import SessionTimeline from './SessionTimeline.svelte';

  const API_BASE = 'http://localhost:3000';

  interface Project {
    id: number;
    name: string | null;
    path: string;
  }

  interface SessionInfo {
    id: string;
    project_id: number;
    status: string;
    summary: string | null;
    started_at: string;
    last_activity: string;
    tool_count: number;
    top_tools: string[];
  }

  interface Goal {
    id: number;
    title: string;
    description: string | null;
    status: string;
    priority: string;
    progress_percent: number;
    project_id: number | null;
    created_at: string;
  }

  interface Task {
    id: number;
    title: string;
    description: string | null;
    status: string;
    priority: string;
    project_id: number | null;
    goal_id: number | null;
    created_at: string;
  }

  // Props
  let {
    currentProject = null,
    onProjectChange = (_p: { name: string; path: string } | null) => {}
  }: {
    currentProject: { name: string; path: string } | null;
    onProjectChange?: (project: { name: string; path: string } | null) => void;
  } = $props();

  // State
  let projects = $state<Project[]>([]);
  let selectedProject = $state<Project | null>(null);
  let sessions = $state<SessionInfo[]>([]);
  let goals = $state<Goal[]>([]);
  let tasks = $state<Task[]>([]);
  let loadingSessions = $state(false);
  let selectedSessionId = $state<string | null>(null);
  let activeTab = $state<'sessions' | 'goals'>('sessions');

  // Load projects
  onMount(async () => {
    try {
      const res = await fetch(`${API_BASE}/api/projects`);
      if (res.ok) {
        const data = await res.json();
        projects = data.data || [];

        // Auto-select current project if available
        if (currentProject) {
          const match = projects.find(p => p.path === currentProject.path);
          if (match) {
            selectProject(match);
          }
        }
      }
    } catch (e) {
      console.error('Failed to load projects:', e);
    }
  });

  // Select project and load sessions
  async function selectProject(project: Project) {
    selectedProject = project;
    selectedSessionId = null;
    loadingSessions = true;

    // Set as active project (applies persona overlay)
    try {
      const setRes = await fetch(`${API_BASE}/api/project/set`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path: project.path, name: project.name }),
      });
      if (setRes.ok) {
        const data = await setRes.json();
        // Notify parent to update header
        onProjectChange(data.data || { name: project.name || 'Unnamed', path: project.path });
      }
    } catch (e) {
      console.error('Failed to set project:', e);
    }

    // Load sessions, goals, and tasks in parallel
    try {
      const [sessionsRes, goalsRes, tasksRes] = await Promise.all([
        fetch(`${API_BASE}/api/sessions?project_id=${project.id}`),
        fetch(`${API_BASE}/api/goals`),
        fetch(`${API_BASE}/api/tasks`),
      ]);

      if (sessionsRes.ok) {
        const data = await sessionsRes.json();
        sessions = data.data || [];
      }

      if (goalsRes.ok) {
        const data = await goalsRes.json();
        // Filter goals for this project (or global goals with null project_id)
        goals = (data.data || []).filter((g: Goal) =>
          g.project_id === project.id || g.project_id === null
        );
      }

      if (tasksRes.ok) {
        const data = await tasksRes.json();
        // Filter tasks for this project
        tasks = (data.data || []).filter((t: Task) =>
          t.project_id === project.id || t.project_id === null
        );
      }
    } catch (e) {
      console.error('Failed to load project data:', e);
      sessions = [];
      goals = [];
      tasks = [];
    }

    loadingSessions = false;
  }

  // Status colors
  function getStatusColor(status: string): string {
    switch (status) {
      case 'completed': return 'var(--green)';
      case 'in_progress': return 'var(--blue)';
      case 'pending': return 'var(--yellow)';
      case 'blocked': return 'var(--red)';
      case 'abandoned': return 'var(--overlay0)';
      default: return 'var(--overlay0)';
    }
  }

  function getPriorityIcon(priority: string): string {
    switch (priority) {
      case 'urgent': return '!!!';
      case 'high': return '!!';
      case 'medium': return '!';
      case 'low': return '-';
      default: return '';
    }
  }

  // Format date
  function formatDate(dateStr: string): string {
    const date = new Date(dateStr.replace(' ', 'T') + 'Z');
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffHours = diffMs / (1000 * 60 * 60);

    if (diffHours < 1) {
      const mins = Math.floor(diffMs / (1000 * 60));
      return `${mins}m ago`;
    } else if (diffHours < 24) {
      return `${Math.floor(diffHours)}h ago`;
    } else if (diffHours < 48) {
      return 'Yesterday';
    } else {
      return date.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
    }
  }

  function formatTime(dateStr: string): string {
    const date = new Date(dateStr.replace(' ', 'T') + 'Z');
    return date.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit', hour12: false });
  }
</script>

<div class="projects-view">
  <!-- Projects Panel -->
  <aside class="projects-panel">
    <div class="panel-header">
      <span class="panel-label">Projects</span>
      <span class="project-count">{projects.length}</span>
    </div>

    <div class="projects-list">
      {#each projects as project (project.id)}
        <button
          class="project-card"
          class:selected={selectedProject?.id === project.id}
          onclick={() => selectProject(project)}
        >
          <div class="project-indicator"></div>
          <div class="project-info">
            <span class="project-name">{project.name || 'Unnamed'}</span>
            <span class="project-path">{project.path}</span>
          </div>
          <svg class="chevron" width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
            <path d="M6 4l4 4-4 4"/>
          </svg>
        </button>
      {/each}

      {#if projects.length === 0}
        <div class="empty-state">
          <span class="empty-icon">⊘</span>
          <span>No projects indexed</span>
        </div>
      {/if}
    </div>
  </aside>

  <!-- Main Panel -->
  <main class="sessions-panel">
    {#if selectedProject}
      <div class="panel-header">
        <div class="header-left">
          <!-- Tab switcher -->
          <div class="tab-switcher">
            <button
              class="tab-btn"
              class:active={activeTab === 'sessions'}
              onclick={() => activeTab = 'sessions'}
            >
              Sessions
              <span class="tab-count">{sessions.length}</span>
            </button>
            <button
              class="tab-btn"
              class:active={activeTab === 'goals'}
              onclick={() => activeTab = 'goals'}
            >
              Goals & Tasks
              <span class="tab-count">{goals.length + tasks.length}</span>
            </button>
          </div>
          <span class="session-context">{selectedProject.name || selectedProject.path}</span>
        </div>
      </div>

      {#if loadingSessions}
        <div class="loading-state">
          <div class="spinner"></div>
          <span>Loading...</span>
        </div>
      {:else if activeTab === 'sessions'}
        <!-- Sessions Tab -->
        {#if sessions.length === 0}
          <div class="empty-state centered">
            <span class="empty-icon">◇</span>
            <span>No sessions recorded</span>
            <span class="empty-hint">Sessions are created when Claude Code connects via MCP</span>
          </div>
        {:else}
          <div class="sessions-timeline">
            {#each sessions as session, i (session.id)}
              <SessionTimeline
                {session}
                isFirst={i === 0}
                isLast={i === sessions.length - 1}
                isExpanded={selectedSessionId === session.id}
                onToggle={() => selectedSessionId = selectedSessionId === session.id ? null : session.id}
                {formatDate}
                {formatTime}
              />
            {/each}
          </div>
        {/if}
      {:else}
        <!-- Goals & Tasks Tab -->
        <div class="goals-tasks-view">
          {#if goals.length === 0 && tasks.length === 0}
            <div class="empty-state centered">
              <span class="empty-icon">*</span>
              <span>No goals or tasks</span>
              <span class="empty-hint">Create goals via MCP tools</span>
            </div>
          {:else}
            <!-- Goals Section -->
            {#if goals.length > 0}
              <div class="section">
                <div class="section-header">
                  <span class="section-label">Goals</span>
                  <span class="section-count">{goals.length}</span>
                </div>
                <div class="goals-list">
                  {#each goals as goal (goal.id)}
                    <div class="goal-card">
                      <div class="goal-header">
                        <span class="priority-icon">{getPriorityIcon(goal.priority)}</span>
                        <span class="goal-title">{goal.title}</span>
                        <span class="status-badge" style="background: color-mix(in srgb, {getStatusColor(goal.status)} 20%, transparent); color: {getStatusColor(goal.status)};">
                          {goal.status.replace('_', ' ')}
                        </span>
                      </div>
                      {#if goal.description}
                        <p class="goal-description">{goal.description}</p>
                      {/if}
                      <div class="goal-progress">
                        <div class="progress-bar">
                          <div class="progress-fill" style="width: {goal.progress_percent}%; background: {getStatusColor(goal.status)};"></div>
                        </div>
                        <span class="progress-label">{goal.progress_percent}%</span>
                      </div>
                    </div>
                  {/each}
                </div>
              </div>
            {/if}

            <!-- Tasks Section -->
            {#if tasks.length > 0}
              <div class="section">
                <div class="section-header">
                  <span class="section-label">Tasks</span>
                  <span class="section-count">{tasks.length}</span>
                </div>
                <div class="tasks-list">
                  {#each tasks as task (task.id)}
                    <div class="task-card" class:completed={task.status === 'completed'}>
                      <div class="task-check" style="border-color: {getStatusColor(task.status)};">
                        {#if task.status === 'completed'}
                          <svg width="10" height="10" viewBox="0 0 16 16" fill="{getStatusColor(task.status)}">
                            <path d="M13.78 4.22a.75.75 0 010 1.06l-7.25 7.25a.75.75 0 01-1.06 0L2.22 9.28a.75.75 0 011.06-1.06L6 10.94l6.72-6.72a.75.75 0 011.06 0z"/>
                          </svg>
                        {/if}
                      </div>
                      <div class="task-content">
                        <div class="task-header">
                          <span class="task-title">{task.title}</span>
                          <span class="task-priority">{getPriorityIcon(task.priority)}</span>
                        </div>
                        {#if task.description}
                          <p class="task-description">{task.description}</p>
                        {/if}
                      </div>
                    </div>
                  {/each}
                </div>
              </div>
            {/if}
          {/if}
        </div>
      {/if}
    {:else}
      <div class="empty-state centered">
        <div class="select-prompt">
          <span class="prompt-icon">←</span>
          <span>Select a project to view details</span>
        </div>
      </div>
    {/if}
  </main>
</div>

<style>
  .projects-view {
    display: flex;
    height: 100%;
    background: var(--base);
  }

  /* Projects Panel */
  .projects-panel {
    width: 320px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--glass-border);
    background: linear-gradient(180deg, var(--mantle) 0%, var(--base) 100%);
  }

  .panel-header {
    padding: 1rem 1.25rem;
    display: flex;
    align-items: center;
    justify-content: space-between;
    border-bottom: 1px solid var(--glass-border);
    background: var(--glass-heavy);
    backdrop-filter: blur(12px);
  }

  .panel-label {
    font-size: 0.6875rem;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--mauve);
  }

  .project-count, .session-count {
    font-size: 0.6875rem;
    font-family: var(--font-mono);
    color: var(--overlay0);
    padding: 0.125rem 0.5rem;
    background: var(--surface0);
    border-radius: 4px;
  }

  .projects-list {
    flex: 1;
    overflow-y: auto;
    padding: 0.75rem;
  }

  .project-card {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.875rem 1rem;
    margin-bottom: 0.375rem;
    border-radius: 10px;
    border: 1px solid transparent;
    background: transparent;
    cursor: pointer;
    transition: all 0.2s cubic-bezier(0.4, 0, 0.2, 1);
    text-align: left;
  }

  .project-card:hover {
    background: var(--surface0);
    border-color: var(--glass-border);
  }

  .project-card.selected {
    background: linear-gradient(135deg, rgba(137, 180, 250, 0.12) 0%, rgba(116, 199, 236, 0.08) 100%);
    border-color: rgba(137, 180, 250, 0.3);
    box-shadow: 0 0 20px rgba(137, 180, 250, 0.1);
  }

  .project-indicator {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--overlay0);
    flex-shrink: 0;
    transition: all 0.2s;
  }

  .project-card.selected .project-indicator {
    background: var(--blue);
    box-shadow: 0 0 8px var(--blue);
  }

  .project-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  .project-name {
    font-size: 0.875rem;
    font-weight: 500;
    color: var(--foreground);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .project-path {
    font-size: 0.6875rem;
    font-family: var(--font-mono);
    color: var(--overlay0);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .chevron {
    flex-shrink: 0;
    color: var(--overlay0);
    opacity: 0;
    transform: translateX(-4px);
    transition: all 0.2s;
  }

  .project-card:hover .chevron,
  .project-card.selected .chevron {
    opacity: 1;
    transform: translateX(0);
  }

  .project-card.selected .chevron {
    color: var(--blue);
  }

  /* Sessions Panel */
  .sessions-panel {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    background:
      radial-gradient(ellipse at 20% 0%, rgba(137, 180, 250, 0.03) 0%, transparent 50%),
      radial-gradient(ellipse at 80% 100%, rgba(203, 166, 247, 0.03) 0%, transparent 50%),
      var(--base);
  }

  .sessions-panel .panel-header {
    background: var(--glass);
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: 1rem;
  }

  .session-context {
    font-size: 0.8125rem;
    font-family: var(--font-mono);
    color: var(--subtext0);
    padding-left: 1rem;
    border-left: 1px solid var(--glass-border);
  }

  .sessions-timeline {
    flex: 1;
    overflow-y: auto;
    padding: 1.5rem 2rem 2rem;
  }

  /* Empty/Loading States */
  .empty-state {
    padding: 2rem;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.5rem;
    color: var(--overlay0);
    font-size: 0.8125rem;
  }

  .empty-state.centered {
    flex: 1;
    justify-content: center;
  }

  .empty-icon {
    font-size: 1.5rem;
    opacity: 0.5;
    margin-bottom: 0.25rem;
  }

  .empty-hint {
    font-size: 0.75rem;
    color: var(--overlay0);
    opacity: 0.7;
  }

  .select-prompt {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 1rem 1.5rem;
    background: var(--surface0);
    border-radius: 10px;
    border: 1px dashed var(--surface1);
  }

  .prompt-icon {
    font-size: 1.25rem;
    color: var(--blue);
    animation: point 1.5s ease-in-out infinite;
  }

  @keyframes point {
    0%, 100% { transform: translateX(0); }
    50% { transform: translateX(-4px); }
  }

  .loading-state {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 1rem;
    color: var(--overlay0);
    font-size: 0.8125rem;
  }

  .spinner {
    width: 24px;
    height: 24px;
    border: 2px solid var(--surface1);
    border-top-color: var(--mauve);
    border-radius: 50%;
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  /* Tab Switcher */
  .tab-switcher {
    display: flex;
    gap: 0.25rem;
    padding: 0.25rem;
    background: var(--mantle);
    border-radius: 8px;
  }

  .tab-btn {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.5rem 0.875rem;
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--muted);
    background: transparent;
    border: none;
    border-radius: 6px;
    cursor: pointer;
    transition: all 0.15s;
  }

  .tab-btn:hover {
    color: var(--foreground);
    background: var(--surface0);
  }

  .tab-btn.active {
    color: var(--foreground);
    background: var(--surface0);
  }

  .tab-count {
    font-family: var(--font-mono);
    font-size: 0.625rem;
    padding: 0.125rem 0.375rem;
    background: var(--surface1);
    border-radius: 4px;
  }

  .tab-btn.active .tab-count {
    background: var(--accent-faded);
    color: var(--accent);
  }

  /* Goals & Tasks View */
  .goals-tasks-view {
    flex: 1;
    overflow-y: auto;
    padding: 1.5rem 2rem;
  }

  .section {
    margin-bottom: 2rem;
  }

  .section-header {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin-bottom: 1rem;
    padding-bottom: 0.5rem;
    border-bottom: 1px solid var(--glass-border);
  }

  .section-label {
    font-size: 0.6875rem;
    font-weight: 700;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    color: var(--mauve);
  }

  .section-count {
    font-family: var(--font-mono);
    font-size: 0.625rem;
    color: var(--overlay0);
    padding: 0.125rem 0.375rem;
    background: var(--surface0);
    border-radius: 4px;
  }

  /* Goals */
  .goals-list {
    display: flex;
    flex-direction: column;
    gap: 0.75rem;
  }

  .goal-card {
    padding: 1rem;
    background: var(--surface0);
    border: 1px solid var(--glass-border);
    border-radius: 10px;
  }

  .goal-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .priority-icon {
    font-family: var(--font-mono);
    font-size: 0.625rem;
    font-weight: 700;
    color: var(--peach);
    min-width: 1.5rem;
  }

  .goal-title {
    flex: 1;
    font-size: 0.875rem;
    font-weight: 500;
    color: var(--foreground);
  }

  .status-badge {
    font-size: 0.625rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    padding: 0.25rem 0.5rem;
    border-radius: 4px;
  }

  .goal-description {
    margin: 0.75rem 0 0;
    font-size: 0.8125rem;
    color: var(--subtext0);
    line-height: 1.5;
  }

  .goal-progress {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin-top: 0.75rem;
  }

  .progress-bar {
    flex: 1;
    height: 4px;
    background: var(--surface1);
    border-radius: 2px;
    overflow: hidden;
  }

  .progress-fill {
    height: 100%;
    border-radius: 2px;
    transition: width 0.3s ease;
  }

  .progress-label {
    font-family: var(--font-mono);
    font-size: 0.6875rem;
    color: var(--overlay0);
    min-width: 2.5rem;
    text-align: right;
  }

  /* Tasks */
  .tasks-list {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .task-card {
    display: flex;
    align-items: flex-start;
    gap: 0.75rem;
    padding: 0.75rem 1rem;
    background: var(--surface0);
    border: 1px solid var(--glass-border);
    border-radius: 8px;
    transition: all 0.15s;
  }

  .task-card:hover {
    border-color: var(--surface1);
  }

  .task-card.completed {
    opacity: 0.6;
  }

  .task-card.completed .task-title {
    text-decoration: line-through;
  }

  .task-check {
    width: 16px;
    height: 16px;
    flex-shrink: 0;
    border: 2px solid var(--overlay0);
    border-radius: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    margin-top: 2px;
  }

  .task-content {
    flex: 1;
    min-width: 0;
  }

  .task-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .task-title {
    font-size: 0.8125rem;
    font-weight: 500;
    color: var(--foreground);
  }

  .task-priority {
    font-family: var(--font-mono);
    font-size: 0.5625rem;
    font-weight: 700;
    color: var(--peach);
  }

  .task-description {
    margin: 0.375rem 0 0;
    font-size: 0.75rem;
    color: var(--subtext0);
    line-height: 1.4;
  }
</style>
