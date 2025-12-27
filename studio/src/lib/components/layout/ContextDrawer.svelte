<script lang="ts">
  import { layoutStore, type DrawerTab } from '$lib/stores/layout.svelte';
  import TimelineTab from '../timeline/TimelineTab.svelte';
  import WorkspaceTab from '../workspace/WorkspaceTab.svelte';
  import OrchestrationTab from '../orchestration/OrchestrationTab.svelte';

  // Tab configuration
  const tabs: { id: DrawerTab; label: string; icon: string }[] = [
    { id: 'orchestration', label: 'Claude', icon: 'terminal' },
    { id: 'timeline', label: 'Timeline', icon: 'clock' },
    { id: 'workspace', label: 'Workspace', icon: 'folder' },
  ];

  // Resize handling
  let isResizing = $state(false);
  let startX = 0;
  let startWidth = 0;

  function startResize(e: MouseEvent) {
    isResizing = true;
    startX = e.clientX;
    startWidth = layoutStore.contextDrawer.width;
    document.addEventListener('mousemove', handleResize);
    document.addEventListener('mouseup', stopResize);
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';
  }

  function handleResize(e: MouseEvent) {
    if (!isResizing) return;
    // Resize from left edge (decreasing X = wider panel)
    const delta = startX - e.clientX;
    layoutStore.setDrawerWidth(startWidth + delta);
  }

  function stopResize() {
    isResizing = false;
    document.removeEventListener('mousemove', handleResize);
    document.removeEventListener('mouseup', stopResize);
    document.body.style.cursor = '';
    document.body.style.userSelect = '';
  }

  function handleTabClick(tabId: DrawerTab) {
    if (layoutStore.contextDrawer.activeTab === tabId && layoutStore.contextDrawer.open) {
      // Toggle closed if clicking active tab
      layoutStore.closeDrawer();
    } else {
      layoutStore.setDrawerTab(tabId);
    }
  }
</script>

{#if layoutStore.isDrawerVisible}
  <!-- Backdrop for mobile -->
  {#if layoutStore.isBottomSheet}
    <button
      class="mobile-backdrop"
      onclick={() => layoutStore.closeDrawer()}
      aria-label="Close panel"
    ></button>
  {/if}

  <aside
    class="context-drawer {layoutStore.isBottomSheet ? 'bottom-sheet' : ''}"
    style={layoutStore.isBottomSheet ? '' : `width: ${layoutStore.contextDrawer.width}px`}
  >
    <!-- Resize handle -->
    <div
      class="resize-handle"
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize drawer"
      onmousedown={startResize}
    ></div>

    <!-- Header with tabs -->
    <div class="drawer-header">
      <div class="drawer-tabs">
        {#each tabs as tab}
          <button
            class="drawer-tab {layoutStore.contextDrawer.activeTab === tab.id ? 'active' : ''}"
            onclick={() => handleTabClick(tab.id)}
          >
            {tab.label}
          </button>
        {/each}
      </div>
      <button
        class="drawer-close"
        onclick={() => layoutStore.closeDrawer()}
        title="Close panel (Cmd+\)"
      >
        <svg class="close-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M18 6L6 18M6 6l12 12" />
        </svg>
      </button>
    </div>

    <!-- Tab content -->
    <div class="drawer-content">
      {#if layoutStore.contextDrawer.activeTab === 'orchestration'}
        <OrchestrationTab />
      {:else if layoutStore.contextDrawer.activeTab === 'timeline'}
        <TimelineTab />
      {:else if layoutStore.contextDrawer.activeTab === 'workspace'}
        <WorkspaceTab />
      {/if}
    </div>
  </aside>
{/if}

<style>
  .context-drawer {
    position: relative;
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--term-bg-secondary);
    border-left: 1px solid var(--term-border);
    min-width: 280px;
    max-width: 600px;
  }

  .resize-handle {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    width: 4px;
    cursor: col-resize;
    background: transparent;
    transition: background 0.15s ease;
    z-index: 10;
  }

  .resize-handle:hover,
  .resize-handle:active {
    background: var(--term-accent);
  }

  .drawer-header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--term-border);
    background: var(--term-bg-secondary);
  }

  .drawer-tabs {
    display: flex;
    gap: 4px;
    flex: 1;
  }

  .drawer-tab {
    padding: 6px 12px;
    font-size: 12px;
    font-weight: 500;
    color: var(--term-text-dim);
    background: transparent;
    border: none;
    border-radius: 4px;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .drawer-tab:hover {
    color: var(--term-text);
    background: var(--term-bg);
  }

  .drawer-tab.active {
    color: var(--term-accent);
    background: var(--term-accent-faded);
  }

  .drawer-close {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border: none;
    background: transparent;
    color: var(--term-text-dim);
    border-radius: 4px;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .drawer-close:hover {
    color: var(--term-text);
    background: var(--term-bg);
  }

  .close-icon {
    width: 14px;
    height: 14px;
  }

  .drawer-content {
    flex: 1;
    overflow: hidden;
  }

  .tab-content {
    height: 100%;
    overflow-y: auto;
  }

  .placeholder {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    padding: 24px;
    text-align: center;
    color: var(--term-text-dim);
  }

  .placeholder-icon {
    width: 48px;
    height: 48px;
    margin-bottom: 16px;
    opacity: 0.5;
  }

  .placeholder-text {
    font-size: 14px;
    font-weight: 500;
    margin-bottom: 4px;
  }

  .placeholder-hint {
    font-size: 12px;
    opacity: 0.7;
  }

  /* Mobile backdrop */
  .mobile-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    z-index: 50;
    border: none;
    cursor: pointer;
  }

  /* Mobile bottom sheet mode */
  .context-drawer.bottom-sheet {
    position: fixed;
    bottom: 0;
    left: 0;
    right: 0;
    height: 70vh;
    max-height: 70vh;
    min-width: unset;
    max-width: unset;
    border-left: none;
    border-top: 1px solid var(--term-border);
    border-radius: 16px 16px 0 0;
    z-index: 51;
    animation: slide-up 0.2s ease-out;
  }

  @keyframes slide-up {
    from {
      transform: translateY(100%);
    }
    to {
      transform: translateY(0);
    }
  }

  /* Swipe handle for mobile */
  .context-drawer.bottom-sheet .drawer-header::before {
    content: '';
    position: absolute;
    top: 8px;
    left: 50%;
    transform: translateX(-50%);
    width: 36px;
    height: 4px;
    background: var(--term-border);
    border-radius: 2px;
  }

  .context-drawer.bottom-sheet .drawer-header {
    position: relative;
    padding-top: 20px;
  }

  /* Hide resize handle on mobile */
  .context-drawer.bottom-sheet .resize-handle {
    display: none;
  }
</style>
