<script lang="ts">
  import ChatPanel from '$lib/components/ChatPanel.svelte';
  import WorkspacePanel from '$lib/components/WorkspacePanel.svelte';

  // Panel visibility state - start collapsed
  let showWorkspace = $state(false);

  function toggleWorkspace() {
    showWorkspace = !showWorkspace;
  }
</script>

<div class="flex h-full relative">
  <!-- Chat Panel (warm, cozy) -->
  <div class="flex-1 min-w-[400px] flex flex-col border-r border-gray-200">
    <ChatPanel />
  </div>

  <!-- Workspace toggle button (when collapsed) -->
  {#if !showWorkspace}
    <button
      onclick={toggleWorkspace}
      class="absolute right-0 top-1/2 -translate-y-1/2 bg-[var(--terminal-bg)] text-[var(--terminal-text)]
             px-2 py-4 rounded-l-lg border border-r-0 border-[var(--terminal-border)]
             hover:bg-[var(--terminal-hover)] transition-colors z-10"
      title="Show workspace terminal"
    >
      <span class="text-xs font-mono writing-mode-vertical">terminal</span>
      <span class="block mt-1">‹</span>
    </button>
  {/if}

  <!-- Workspace Panel (sexy terminal) -->
  {#if showWorkspace}
    <div class="w-[600px] flex-shrink-0 bg-[var(--terminal-bg)] flex flex-col">
      <WorkspacePanel onCollapse={toggleWorkspace} />
    </div>
  {/if}
</div>

<style>
  .writing-mode-vertical {
    writing-mode: vertical-rl;
    text-orientation: mixed;
  }
</style>
