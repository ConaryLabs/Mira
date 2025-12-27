<script lang="ts">
  import { layoutStore, type LeftNavState } from '$lib/stores/layout.svelte';
  import { currentTheme } from '$lib/stores/theme';
  import type { StatusResponse } from '$lib/api/client';
  import ProjectSelector from '../sidebar/ProjectSelector.svelte';
  import StatusDashboard from '../sidebar/StatusDashboard.svelte';
  import ThemePicker from '../sidebar/ThemePicker.svelte';

  interface Props {
    connected?: boolean;
    status?: StatusResponse | null;
  }

  let { connected = false, status = null }: Props = $props();

  // Reactive: current nav state
  const navState = $derived(layoutStore.leftNav);
  const isSettings = $derived(navState === 'settings');

  function cycleTheme() {
    const themes = ['dracula', 'monokai', 'nord', 'solarized', 'github-dark', 'tokyo-night'] as const;
    const current = $currentTheme;
    const currentIndex = themes.indexOf(current as typeof themes[number]);
    const nextIndex = (currentIndex + 1) % themes.length;
    currentTheme.set(themes[nextIndex]);
  }

  function handleSettingsClick() {
    layoutStore.toggleSettings();
  }
</script>

<nav class="nav-rail" class:expanded={isSettings}>
  <!-- Header -->
  <div class="nav-rail-header">
    {#if isSettings}
      <div class="header-expanded">
        <div class="logo-row">
          <span class="logo" title={connected ? 'Connected' : 'Disconnected'}>
            <span class="logo-text">M</span>
            <span class="status-dot {connected ? 'connected' : 'disconnected'}"></span>
          </span>
          <span class="brand-text">Mira</span>
        </div>
        <button
          class="collapse-btn"
          onclick={handleSettingsClick}
          title="Close settings"
        >
          <svg class="nav-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M11 19l-7-7 7-7m8 14l-7-7 7-7" />
          </svg>
        </button>
      </div>
    {:else}
      <span class="logo" title={connected ? 'Connected' : 'Disconnected'}>
        <span class="logo-text">M</span>
        <span class="status-dot {connected ? 'connected' : 'disconnected'}"></span>
      </span>
    {/if}
  </div>

  {#if isSettings}
    <!-- Settings content (expanded view) -->
    <div class="settings-content">
      <ProjectSelector />
      <StatusDashboard {status} />
      <ThemePicker />
    </div>

    <!-- Footer in settings mode -->
    <div class="settings-footer">
      <span class="model-badge">DeepSeek V3.2</span>
    </div>
  {:else}
    <!-- Collapsed nav items -->
    <div class="nav-rail-items">
      <!-- Timeline -->
      <button
        class="nav-item {layoutStore.contextDrawer.activeTab === 'timeline' && layoutStore.contextDrawer.open ? 'active' : ''}"
        onclick={() => layoutStore.setDrawerTab('timeline')}
        title="Timeline (tool activity)"
      >
        <svg class="nav-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M12 8v4l3 3" />
          <circle cx="12" cy="12" r="9" />
        </svg>
      </button>

      <!-- Workspace/Artifacts -->
      <button
        class="nav-item {layoutStore.contextDrawer.activeTab === 'workspace' && layoutStore.contextDrawer.open ? 'active' : ''}"
        onclick={() => layoutStore.setDrawerTab('workspace')}
        title="Workspace (files & artifacts)"
      >
        <svg class="nav-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M3 7v10a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-6l-2-2H5a2 2 0 0 0-2 2z" />
        </svg>
      </button>
    </div>

    <!-- Spacer -->
    <div class="flex-1"></div>

    <!-- Bottom actions -->
    <div class="nav-rail-footer">
      <!-- Theme toggle -->
      <button
        class="nav-item"
        onclick={cycleTheme}
        title="Cycle theme ({$currentTheme})"
      >
        <svg class="nav-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <circle cx="12" cy="12" r="5" />
          <path d="M12 1v2M12 21v2M4.22 4.22l1.42 1.42M18.36 18.36l1.42 1.42M1 12h2M21 12h2M4.22 19.78l1.42-1.42M18.36 5.64l1.42-1.42" />
        </svg>
      </button>

      <!-- Settings -->
      <button
        class="nav-item {isSettings ? 'active' : ''}"
        onclick={handleSettingsClick}
        title="Settings"
      >
        <svg class="nav-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <circle cx="12" cy="12" r="3" />
          <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
        </svg>
      </button>
    </div>
  {/if}
</nav>

<style>
  .nav-rail {
    display: flex;
    flex-direction: column;
    width: 48px;
    min-width: 48px;
    height: 100%;
    background: var(--term-bg-secondary);
    border-right: 1px solid var(--term-border);
    padding: 8px 0;
    transition: width 0.2s ease;
    overflow: hidden;
  }

  .nav-rail.expanded {
    width: 280px;
    min-width: 280px;
  }

  .nav-rail-header {
    display: flex;
    justify-content: center;
    padding: 8px;
    margin-bottom: 8px;
  }

  .header-expanded {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: 0 4px;
  }

  .logo-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .brand-text {
    color: var(--term-text);
    font-family: var(--font-mono);
    font-size: 14px;
    font-weight: 500;
  }

  .collapse-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: 6px;
    background: transparent;
    border: none;
    color: var(--term-text-dim);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .collapse-btn:hover {
    background: var(--term-bg);
    color: var(--term-text);
  }

  .logo {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border-radius: 8px;
    background: var(--term-accent);
    color: var(--term-bg);
    font-weight: bold;
    font-size: 16px;
    font-family: var(--font-mono);
    flex-shrink: 0;
  }

  .logo-text {
    position: relative;
    z-index: 1;
  }

  .status-dot {
    position: absolute;
    bottom: -2px;
    right: -2px;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    border: 2px solid var(--term-bg-secondary);
  }

  .status-dot.connected {
    background: var(--term-success);
  }

  .status-dot.disconnected {
    background: var(--term-error);
  }

  /* Settings content */
  .settings-content {
    flex: 1;
    overflow-y: auto;
    padding: 0 12px;
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  .settings-footer {
    padding: 12px;
    border-top: 1px solid var(--term-border);
    text-align: center;
  }

  .model-badge {
    font-size: 11px;
    color: var(--term-text-dim);
    font-family: var(--font-mono);
  }

  /* Collapsed nav items */
  .nav-rail-items {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 0 8px;
  }

  .nav-rail-footer {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 0 8px;
    margin-top: auto;
  }

  .nav-item {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border-radius: 8px;
    background: transparent;
    border: none;
    color: var(--term-text-dim);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .nav-item:hover {
    background: var(--term-bg);
    color: var(--term-text);
  }

  .nav-item.active {
    background: var(--term-accent-faded);
    color: var(--term-accent);
  }

  .nav-icon {
    width: 18px;
    height: 18px;
  }

  .collapse-btn .nav-icon {
    width: 16px;
    height: 16px;
  }
</style>
