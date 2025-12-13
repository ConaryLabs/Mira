<script lang="ts">
  import ChatPanel from '$lib/components/ChatPanel.svelte';
  import WorkspacePanel from '$lib/components/WorkspacePanel.svelte';

  // Panel visibility state
  let showWorkspace = $state(true);
</script>

<div class="flex h-full">
  <!-- Chat Panel (warm, cozy) -->
  <div class="flex-1 min-w-[400px] flex flex-col border-r border-gray-200">
    <ChatPanel />
  </div>

  <!-- Workspace Panel (sexy terminal) -->
  {#if showWorkspace}
    <div class="w-[600px] flex-shrink-0 bg-[var(--terminal-bg)]">
      <WorkspacePanel />
    </div>
  {/if}
</div>
