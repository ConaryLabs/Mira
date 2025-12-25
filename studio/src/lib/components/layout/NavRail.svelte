<script lang="ts">
  import { layoutStore } from '$lib/stores/layout.svelte';
  import { currentTheme } from '$lib/stores/theme';

  // Status indicator
  interface Props {
    connected?: boolean;
    onSettingsClick?: () => void;
  }

  let { connected = false, onSettingsClick }: Props = $props();

  function cycleTheme() {
    const themes = ['dracula', 'monokai', 'nord', 'solarized', 'github-dark', 'tokyo-night'] as const;
    const current = $currentTheme;
    const currentIndex = themes.indexOf(current as typeof themes[number]);
    const nextIndex = (currentIndex + 1) % themes.length;
    currentTheme.set(themes[nextIndex]);
  }
</script>

<nav class="nav-rail">
  <!-- Logo/Status -->
  <div class="nav-rail-header">
    <span class="logo" title={connected ? 'Connected' : 'Disconnected'}>
      <span class="logo-text">M</span>
      <span class="status-dot {connected ? 'connected' : 'disconnected'}"></span>
    </span>
  </div>

  <!-- Main nav items -->
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

    <!-- Advisory/Council -->
    <button
      class="nav-item {layoutStore.contextDrawer.activeTab === 'advisory' && layoutStore.contextDrawer.open ? 'active' : ''}"
      onclick={() => layoutStore.setDrawerTab('advisory')}
      title="Advisory Council (session history)"
    >
      <svg class="nav-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2" />
        <circle cx="9" cy="7" r="4" />
        <path d="M23 21v-2a4 4 0 0 0-3-3.87" />
        <path d="M16 3.13a4 4 0 0 1 0 7.75" />
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
      class="nav-item {layoutStore.settingsOpen ? 'active' : ''}"
      onclick={onSettingsClick}
      title="Settings"
    >
      <svg class="nav-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <circle cx="12" cy="12" r="3" />
        <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
      </svg>
    </button>
  </div>
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
  }

  .nav-rail-header {
    display: flex;
    justify-content: center;
    padding: 8px 0;
    margin-bottom: 8px;
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
</style>
