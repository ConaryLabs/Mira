<script lang="ts">
  import { settings } from '$lib/stores/settings';
  import type { StatusResponse } from '$lib/api/client';
  import ProjectInput from './ProjectInput.svelte';
  import StatusDashboard from './StatusDashboard.svelte';
  import ThemePicker from './ThemePicker.svelte';

  interface Props {
    status: StatusResponse | null;
    onClose?: () => void;
    isMobile?: boolean;
  }

  let { status, onClose, isMobile = false }: Props = $props();

  function toggleCollapsed() {
    settings.setSidebarCollapsed(!$settings.sidebarCollapsed);
  }
</script>

<aside
  class="h-full flex flex-col bg-[var(--term-bg-secondary)] border-r border-[var(--term-border)] transition-all duration-200 {isMobile ? 'w-72' : ($settings.sidebarCollapsed ? 'w-12' : 'w-72')}"
>
  <!-- Header -->
  <div class="flex items-center justify-between p-3 border-b border-[var(--term-border)]">
    {#if isMobile || !$settings.sidebarCollapsed}
      <div class="flex items-center gap-2">
        <span class="text-[var(--term-accent)] font-mono font-bold">M</span>
        <span class="text-[var(--term-text)] font-mono text-sm">Mira</span>
      </div>
    {/if}
    {#if isMobile}
      <!-- Mobile close button -->
      <button
        onclick={onClose}
        class="p-1.5 rounded hover:bg-[var(--term-bg)] text-[var(--term-text-dim)] hover:text-[var(--term-text)] transition-colors"
        title="Close menu"
      >
        <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
        </svg>
      </button>
    {:else}
      <button
        onclick={toggleCollapsed}
        class="p-1.5 rounded hover:bg-[var(--term-bg)] text-[var(--term-text-dim)] hover:text-[var(--term-text)] transition-colors {$settings.sidebarCollapsed ? 'mx-auto' : ''}"
        title={$settings.sidebarCollapsed ? 'Expand sidebar' : 'Collapse sidebar'}
      >
        <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          {#if $settings.sidebarCollapsed}
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 5l7 7-7 7M5 5l7 7-7 7" />
          {:else}
            <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M11 19l-7-7 7-7m8 14l-7-7 7-7" />
          {/if}
        </svg>
      </button>
    {/if}
  </div>

  <!-- Settings sections (hidden when collapsed, always show on mobile) -->
  {#if isMobile || !$settings.sidebarCollapsed}
    <div class="flex-1 overflow-y-auto p-4 space-y-6">
      <ProjectInput />
      <StatusDashboard {status} />
      <ThemePicker />
    </div>

    <!-- Footer -->
    <div class="p-3 border-t border-[var(--term-border)] text-center">
      <span class="text-xs text-[var(--term-text-dim)] font-mono">DeepSeek V3.2</span>
    </div>
  {:else if !isMobile}
    <!-- Collapsed icons -->
    <div class="flex-1 flex flex-col items-center py-4 space-y-4">
      <button
        onclick={toggleCollapsed}
        class="p-2 rounded hover:bg-[var(--term-bg)] text-[var(--term-text-dim)] hover:text-[var(--term-accent)] transition-colors"
        title="Project"
      >
        <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
        </svg>
      </button>
      <button
        onclick={toggleCollapsed}
        class="p-2 rounded hover:bg-[var(--term-bg)] text-[var(--term-text-dim)] hover:text-[var(--term-accent)] transition-colors"
        title="Theme"
      >
        <svg class="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M7 21a4 4 0 01-4-4V5a2 2 0 012-2h4a2 2 0 012 2v12a4 4 0 01-4 4zm0 0h12a2 2 0 002-2v-4a2 2 0 00-2-2h-2.343M11 7.343l1.657-1.657a2 2 0 012.828 0l2.829 2.829a2 2 0 010 2.828l-8.486 8.485M7 17h.01" />
        </svg>
      </button>
      <!-- Status indicator -->
      <div class="mt-auto pb-2">
        <span
          class="w-2 h-2 rounded-full block {status?.status === 'ok' ? 'bg-[var(--term-success)]' : 'bg-[var(--term-error)]'}"
          title={status?.status === 'ok' ? 'Connected' : 'Disconnected'}
        ></span>
      </div>
    </div>
  {/if}
</aside>
