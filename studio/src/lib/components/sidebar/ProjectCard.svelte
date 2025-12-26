<script lang="ts">
  import type { ProjectInfo } from '$lib/stores/settings';
  import { settings } from '$lib/stores/settings';

  interface Props {
    project: ProjectInfo;
    isActive: boolean;
    onSelect: (path: string) => void;
    onRemove: (path: string) => void;
  }

  let { project, isActive, onSelect, onRemove }: Props = $props();

  function formatRelativeTime(timestamp?: number): string {
    if (!timestamp) return '';
    const diff = Date.now() - timestamp;
    const minutes = Math.floor(diff / 60000);
    const hours = Math.floor(diff / 3600000);
    const days = Math.floor(diff / 86400000);

    if (minutes < 1) return 'just now';
    if (minutes < 60) return `${minutes}m ago`;
    if (hours < 24) return `${hours}h ago`;
    if (days < 7) return `${days}d ago`;
    return new Date(timestamp).toLocaleDateString();
  }

  function handlePin(e: MouseEvent) {
    e.stopPropagation();
    settings.togglePinned(project.path);
  }

  function handleRemove(e: MouseEvent) {
    e.stopPropagation();
    onRemove(project.path);
  }
</script>

<div
  class="project-card"
  class:active={isActive}
  role="button"
  tabindex="0"
  onclick={() => onSelect(project.path)}
  onkeydown={(e) => e.key === 'Enter' && onSelect(project.path)}
>
  <div class="card-main">
    <div class="card-icon">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M3 7v10a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-6l-2-2H5a2 2 0 0 0-2 2z" />
      </svg>
    </div>
    <div class="card-content">
      <div class="card-name">{project.name}</div>
      <div class="card-path">{project.path}</div>
    </div>
    {#if project.pinned}
      <div class="pin-indicator" title="Pinned">
        <svg viewBox="0 0 16 16" fill="currentColor">
          <path d="M4.456.734a1.75 1.75 0 012.826.504l.613 1.327a3.08 3.08 0 002.084 1.707l2.454.584c1.332.317 1.8 1.972.832 2.94L11.06 10l3.72 3.72a.75.75 0 11-1.06 1.06L10 11.06l-2.204 2.205c-.968.968-2.623.5-2.94-.832l-.584-2.454a3.08 3.08 0 00-1.707-2.084l-1.327-.613a1.75 1.75 0 01-.504-2.826L4.456.734z"/>
        </svg>
      </div>
    {/if}
  </div>

  <div class="card-meta">
    {#if project.lastActivity}
      <span class="last-activity">{formatRelativeTime(project.lastActivity)}</span>
    {/if}
    <div class="card-actions">
      <button
        class="action-btn"
        class:pinned={project.pinned}
        onclick={handlePin}
        title={project.pinned ? 'Unpin' : 'Pin to top'}
      >
        <svg viewBox="0 0 16 16" fill="currentColor">
          <path d="M4.456.734a1.75 1.75 0 012.826.504l.613 1.327a3.08 3.08 0 002.084 1.707l2.454.584c1.332.317 1.8 1.972.832 2.94L11.06 10l3.72 3.72a.75.75 0 11-1.06 1.06L10 11.06l-2.204 2.205c-.968.968-2.623.5-2.94-.832l-.584-2.454a3.08 3.08 0 00-1.707-2.084l-1.327-.613a1.75 1.75 0 01-.504-2.826L4.456.734z"/>
        </svg>
      </button>
      <button
        class="action-btn remove"
        onclick={handleRemove}
        title="Remove project"
      >
        <svg viewBox="0 0 16 16" fill="currentColor">
          <path d="M3.72 3.72a.75.75 0 011.06 0L8 6.94l3.22-3.22a.75.75 0 111.06 1.06L9.06 8l3.22 3.22a.75.75 0 11-1.06 1.06L8 9.06l-3.22 3.22a.75.75 0 01-1.06-1.06L6.94 8 3.72 4.78a.75.75 0 010-1.06z"/>
        </svg>
      </button>
    </div>
  </div>
</div>

<style>
  .project-card {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 100%;
    padding: 10px;
    background: var(--term-bg);
    border: 1px solid var(--term-border);
    border-radius: 8px;
    cursor: pointer;
    text-align: left;
    transition: all 0.15s ease;
  }

  .project-card:hover {
    border-color: var(--term-accent);
    background: var(--term-bg-secondary);
  }

  .project-card.active {
    border-color: var(--term-accent);
    background: var(--term-accent-faded);
  }

  .card-main {
    display: flex;
    align-items: flex-start;
    gap: 8px;
  }

  .card-icon {
    flex-shrink: 0;
    width: 24px;
    height: 24px;
    color: var(--term-accent);
  }

  .card-icon svg {
    width: 100%;
    height: 100%;
  }

  .card-content {
    flex: 1;
    min-width: 0;
  }

  .card-name {
    font-family: var(--font-mono);
    font-size: 13px;
    font-weight: 500;
    color: var(--term-text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .card-path {
    font-size: 10px;
    color: var(--term-text-dim);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .pin-indicator {
    flex-shrink: 0;
    width: 12px;
    height: 12px;
    color: var(--term-accent);
  }

  .pin-indicator svg {
    width: 100%;
    height: 100%;
  }

  .card-meta {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-left: 32px;
  }

  .last-activity {
    font-size: 10px;
    color: var(--term-text-dim);
  }

  .card-actions {
    display: flex;
    gap: 4px;
    opacity: 0;
    transition: opacity 0.15s ease;
  }

  .project-card:hover .card-actions {
    opacity: 1;
  }

  .action-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--term-text-dim);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .action-btn:hover {
    background: var(--term-bg);
    color: var(--term-text);
  }

  .action-btn.pinned {
    color: var(--term-accent);
  }

  .action-btn.remove:hover {
    color: var(--term-error);
  }

  .action-btn svg {
    width: 12px;
    height: 12px;
  }
</style>
