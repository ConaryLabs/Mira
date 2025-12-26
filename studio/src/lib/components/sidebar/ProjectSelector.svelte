<script lang="ts">
  import { settings } from '$lib/stores/settings';
  import ProjectCard from './ProjectCard.svelte';

  let showAddForm = $state(false);
  let newPath = $state('');
  let inputEl: HTMLInputElement;

  // Get sorted projects from settings
  const projects = $derived(settings.getSortedProjects());

  function handleSelect(path: string) {
    settings.addProject(path);
  }

  function handleRemove(path: string) {
    settings.removeFromHistory(path);
  }

  function handleAddProject() {
    if (newPath.trim()) {
      settings.addProject(newPath.trim());
      newPath = '';
      showAddForm = false;
    }
  }

  function toggleAddForm() {
    showAddForm = !showAddForm;
    if (showAddForm) {
      // Focus input after render
      requestAnimationFrame(() => inputEl?.focus());
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      handleAddProject();
    }
    if (e.key === 'Escape') {
      newPath = '';
      showAddForm = false;
    }
  }
</script>

<div class="project-selector">
  <div class="selector-header">
    <span class="section-label">Projects</span>
    <button
      class="add-btn"
      class:active={showAddForm}
      onclick={toggleAddForm}
      title="Add project"
    >
      <svg viewBox="0 0 16 16" fill="currentColor">
        <path d="M7.75 2a.75.75 0 01.75.75V7h4.25a.75.75 0 010 1.5H8.5v4.25a.75.75 0 01-1.5 0V8.5H2.75a.75.75 0 010-1.5H7V2.75A.75.75 0 017.75 2z"/>
      </svg>
    </button>
  </div>

  {#if showAddForm}
    <div class="add-form">
      <input
        bind:this={inputEl}
        type="text"
        bind:value={newPath}
        placeholder="/path/to/project"
        onkeydown={handleKeydown}
        class="path-input"
      />
      <button class="confirm-btn" onclick={handleAddProject} disabled={!newPath.trim()}>
        Add
      </button>
    </div>
  {/if}

  <div class="projects-list">
    {#if projects.length === 0}
      <div class="empty-state">
        <p>No projects yet</p>
        <p class="hint">Add a project path to get started</p>
      </div>
    {:else}
      {#each projects as project (project.path)}
        <ProjectCard
          {project}
          isActive={project.path === $settings.projectPath}
          onSelect={handleSelect}
          onRemove={handleRemove}
        />
      {/each}
    {/if}
  </div>
</div>

<style>
  .project-selector {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .selector-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .section-label {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--term-text-dim);
  }

  .add-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    padding: 0;
    background: transparent;
    border: 1px solid var(--term-border);
    border-radius: 4px;
    color: var(--term-text-dim);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .add-btn:hover,
  .add-btn.active {
    background: var(--term-accent-faded);
    border-color: var(--term-accent);
    color: var(--term-accent);
  }

  .add-btn svg {
    width: 12px;
    height: 12px;
  }

  .add-form {
    display: flex;
    gap: 6px;
  }

  .path-input {
    flex: 1;
    background: var(--term-bg);
    border: 1px solid var(--term-border);
    border-radius: 4px;
    padding: 6px 8px;
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--term-text);
  }

  .path-input:focus {
    outline: none;
    border-color: var(--term-accent);
  }

  .path-input::placeholder {
    color: var(--term-text-dim);
  }

  .confirm-btn {
    padding: 6px 12px;
    background: var(--term-accent);
    border: none;
    border-radius: 4px;
    font-size: 11px;
    font-weight: 500;
    color: var(--term-bg);
    cursor: pointer;
    transition: opacity 0.15s ease;
  }

  .confirm-btn:hover:not(:disabled) {
    opacity: 0.9;
  }

  .confirm-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .projects-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-height: 240px;
    overflow-y: auto;
  }

  .empty-state {
    padding: 16px;
    text-align: center;
    color: var(--term-text-dim);
  }

  .empty-state p {
    margin: 0;
    font-size: 12px;
  }

  .empty-state .hint {
    margin-top: 4px;
    font-size: 10px;
    opacity: 0.7;
  }
</style>
