<script lang="ts">
  import { toolActivityStore, type ToolCategory, type ToolStatus } from '$lib/stores/toolActivity.svelte';
  import TimelineCard from './TimelineCard.svelte';

  // Filter options
  const categories: { id: ToolCategory | 'all'; label: string }[] = [
    { id: 'all', label: 'All' },
    { id: 'file', label: 'File' },
    { id: 'shell', label: 'Shell' },
    { id: 'memory', label: 'Memory' },
    { id: 'web', label: 'Web' },
    { id: 'git', label: 'Git' },
    { id: 'mira', label: 'Mira' },
  ];

  let selectedCategory = $state<ToolCategory | 'all'>('all');

  function setCategory(cat: ToolCategory | 'all') {
    selectedCategory = cat;
    if (cat === 'all') {
      toolActivityStore.clearFilter();
    } else {
      toolActivityStore.setFilter({ category: cat });
    }
  }

  // Auto-scroll to bottom when new tools appear
  let scrollContainer: HTMLElement;
  let prevCount = 0;

  $effect(() => {
    const count = toolActivityStore.order.length;
    if (count > prevCount && scrollContainer) {
      // New item added, scroll to bottom
      scrollContainer.scrollTop = scrollContainer.scrollHeight;
    }
    prevCount = count;
  });
</script>

<div class="timeline-tab">
  <!-- Header with filters -->
  <div class="timeline-header">
    <div class="filter-row">
      {#each categories as cat}
        <button
          class="filter-btn {selectedCategory === cat.id ? 'active' : ''}"
          onclick={() => setCategory(cat.id)}
        >
          {cat.label}
        </button>
      {/each}
    </div>

    <!-- Active count badge -->
    {#if toolActivityStore.activeCount > 0}
      <div class="active-badge">
        <div class="pulse"></div>
        <span>{toolActivityStore.activeCount} running</span>
      </div>
    {/if}
  </div>

  <!-- Timeline feed -->
  <div class="timeline-feed" bind:this={scrollContainer}>
    {#if toolActivityStore.filteredCalls.length === 0}
      <div class="empty-state">
        <svg class="empty-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
          <path d="M12 8v4l3 3" />
          <circle cx="12" cy="12" r="9" />
        </svg>
        <p class="empty-text">No tool activity yet</p>
        <p class="empty-hint">Tool calls will appear here as they execute</p>
      </div>
    {:else}
      {#each toolActivityStore.filteredCalls as call (call.callId)}
        <TimelineCard {call} />
      {/each}
    {/if}
  </div>
</div>

<style>
  .timeline-tab {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .timeline-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 12px;
    border-bottom: 1px solid var(--term-border);
    background: var(--term-bg-secondary);
  }

  .filter-row {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
  }

  .filter-btn {
    padding: 4px 8px;
    font-size: 11px;
    font-weight: 500;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--term-text-dim);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .filter-btn:hover {
    background: var(--term-bg);
    color: var(--term-text);
  }

  .filter-btn.active {
    background: var(--term-accent-faded);
    color: var(--term-accent);
  }

  .active-badge {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px;
    background: var(--term-accent-faded);
    border-radius: 4px;
    font-size: 11px;
    color: var(--term-accent);
  }

  .pulse {
    width: 8px;
    height: 8px;
    background: var(--term-accent);
    border-radius: 50%;
    animation: pulse 1.5s ease-in-out infinite;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; transform: scale(1); }
    50% { opacity: 0.5; transform: scale(0.9); }
  }

  .timeline-feed {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    padding: 24px;
    text-align: center;
    color: var(--term-text-dim);
  }

  .empty-icon {
    width: 48px;
    height: 48px;
    margin-bottom: 16px;
    opacity: 0.5;
  }

  .empty-text {
    font-size: 14px;
    font-weight: 500;
    margin: 0 0 4px 0;
  }

  .empty-hint {
    font-size: 12px;
    margin: 0;
    opacity: 0.7;
  }
</style>
