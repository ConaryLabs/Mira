<script lang="ts">
  /**
   * WorkspaceTab - Artifacts and working files panel
   *
   * Shows files read, written, and modified during the session
   * Grouped by action type with counts
   */

  import { artifactStore, type ArtifactAction } from '$lib/stores/artifacts.svelte';
  import ArtifactCard from './ArtifactCard.svelte';

  // Filter state
  let filter = $state<ArtifactAction | 'all'>('all');

  // Filtered artifacts
  const filteredArtifacts = $derived(
    filter === 'all'
      ? artifactStore.artifacts
      : artifactStore.artifacts.filter(a => a.action === filter)
  );

  const filterOptions: Array<{ value: ArtifactAction | 'all'; label: string }> = [
    { value: 'all', label: 'All' },
    { value: 'modified', label: 'Modified' },
    { value: 'created', label: 'Created' },
    { value: 'read', label: 'Read' },
  ];
</script>

<div class="workspace-tab">
  <!-- Header with counts -->
  <div class="tab-header">
    <div class="filter-pills">
      {#each filterOptions as option}
        <button
          class="filter-pill {filter === option.value ? 'active' : ''}"
          onclick={() => filter = option.value}
        >
          {option.label}
          {#if option.value === 'all'}
            <span class="count">{artifactStore.counts.total}</span>
          {:else if option.value === 'modified'}
            <span class="count">{artifactStore.counts.modified}</span>
          {:else if option.value === 'created'}
            <span class="count">{artifactStore.counts.created}</span>
          {:else if option.value === 'read'}
            <span class="count">{artifactStore.counts.read}</span>
          {/if}
        </button>
      {/each}
    </div>

    {#if artifactStore.counts.total > 0}
      <button class="clear-btn" onclick={() => artifactStore.clear()}>
        Clear
      </button>
    {/if}
  </div>

  <!-- Artifact list -->
  <div class="artifact-list">
    {#if filteredArtifacts.length === 0}
      <div class="empty-state">
        {#if filter === 'all'}
          <p>No artifacts yet.</p>
          <p class="hint">Files will appear here as they are read, created, or modified.</p>
        {:else}
          <p>No {filter} artifacts.</p>
        {/if}
      </div>
    {:else}
      {#each filteredArtifacts as artifact (artifact.id)}
        <ArtifactCard {artifact} />
      {/each}
    {/if}
  </div>
</div>

<style>
  .workspace-tab {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .tab-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px;
    border-bottom: 1px solid var(--term-border);
    flex-shrink: 0;
  }

  .filter-pills {
    display: flex;
    gap: 4px;
  }

  .filter-pill {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 8px;
    background: var(--term-bg-secondary);
    border: 1px solid var(--term-border);
    border-radius: 12px;
    font-size: 11px;
    color: var(--term-text-dim);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .filter-pill:hover {
    border-color: var(--term-accent);
  }

  .filter-pill.active {
    background: var(--term-accent);
    border-color: var(--term-accent);
    color: var(--term-bg);
  }

  .filter-pill .count {
    font-family: var(--font-mono);
    font-size: 10px;
    padding: 1px 4px;
    background: var(--term-bg);
    border-radius: 6px;
    color: var(--term-text-dim);
  }

  .filter-pill.active .count {
    background: rgba(0, 0, 0, 0.2);
    color: var(--term-bg);
  }

  .clear-btn {
    padding: 4px 8px;
    background: transparent;
    border: 1px solid var(--term-border);
    border-radius: 4px;
    font-size: 11px;
    color: var(--term-text-dim);
    cursor: pointer;
  }

  .clear-btn:hover {
    border-color: var(--term-error);
    color: var(--term-error);
  }

  .artifact-list {
    flex: 1;
    overflow-y: auto;
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--term-text-dim);
    text-align: center;
    padding: 24px;
  }

  .empty-state p {
    margin: 0;
  }

  .empty-state .hint {
    font-size: 12px;
    margin-top: 8px;
    opacity: 0.7;
  }
</style>
