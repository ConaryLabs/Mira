<script lang="ts">
  import { onMount } from 'svelte';
  import { layoutStore } from '$lib/stores/layout.svelte';
  import NavRail from './NavRail.svelte';
  import ContextDrawer from './ContextDrawer.svelte';
  import SettingsSidebar from '../sidebar/SettingsSidebar.svelte';
  import type { StatusResponse } from '$lib/api/client';

  interface Props {
    apiStatus?: StatusResponse | null;
    children?: import('svelte').Snippet;
  }

  let { apiStatus = null, children }: Props = $props();

  // Keyboard shortcuts
  function handleKeydown(event: KeyboardEvent) {
    // Cmd/Ctrl + \ - Toggle drawer
    if ((event.metaKey || event.ctrlKey) && event.key === '\\') {
      event.preventDefault();
      layoutStore.toggleDrawer();
      return;
    }

    // Cmd/Ctrl + , - Toggle settings
    if ((event.metaKey || event.ctrlKey) && event.key === ',') {
      event.preventDefault();
      layoutStore.toggleSettings();
      return;
    }
  }

  onMount(() => {
    // Initialize layout (responsive breakpoints)
    const cleanup = layoutStore.init();

    // Add keyboard listener
    window.addEventListener('keydown', handleKeydown);

    return () => {
      cleanup?.();
      window.removeEventListener('keydown', handleKeydown);
    };
  });

  function handleSettingsClick() {
    layoutStore.toggleSettings();
  }
</script>

<div class="app-shell">
  <!-- Left Navigation Rail -->
  {#if !layoutStore.isMobile}
    <NavRail
      connected={apiStatus?.status === 'ok'}
      onSettingsClick={handleSettingsClick}
    />
  {/if}

  <!-- Mobile header -->
  {#if layoutStore.isMobile}
    <header class="mobile-header">
      <button
        class="mobile-menu-btn"
        onclick={handleSettingsClick}
        aria-label="Open menu"
      >
        <svg class="menu-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M4 6h16M4 12h16M4 18h16" />
        </svg>
      </button>
      <span class="mobile-logo">
        <span class="logo-accent">M</span>
        <span class="logo-text">Mira</span>
      </span>
      <span
        class="status-indicator {apiStatus?.status === 'ok' ? 'connected' : 'disconnected'}"
        title={apiStatus?.status === 'ok' ? 'Connected' : 'Disconnected'}
      ></span>
      <button
        class="mobile-panel-btn"
        onclick={() => layoutStore.toggleDrawer()}
        aria-label="Open panel"
      >
        <svg class="panel-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M4 5a1 1 0 0 1 1-1h14a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V5z" />
          <path d="M15 4v16" />
        </svg>
      </button>
    </header>
  {/if}

  <!-- Main content area -->
  <main class="app-main">
    {#if children}
      {@render children()}
    {/if}
  </main>

  <!-- Right Context Drawer -->
  <ContextDrawer />

  <!-- Settings Sidebar (modal on desktop, slide-over on mobile) -->
  {#if layoutStore.settingsOpen}
    <!-- Backdrop -->
    <button
      class="settings-backdrop"
      onclick={() => layoutStore.closeSettings()}
      aria-label="Close settings"
    ></button>
    <div class="settings-panel {layoutStore.isMobile ? 'mobile' : 'desktop'}">
      <SettingsSidebar
        status={apiStatus}
        onClose={() => layoutStore.closeSettings()}
        isMobile={layoutStore.isMobile}
      />
    </div>
  {/if}
</div>

<style>
  .app-shell {
    display: flex;
    height: 100%;
    width: 100%;
    overflow: hidden;
    background: var(--term-bg);
  }

  .app-main {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    overflow: hidden;
  }

  /* Mobile header */
  .mobile-header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    background: var(--term-bg-secondary);
    border-bottom: 1px solid var(--term-border);
  }

  .mobile-menu-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border: none;
    background: transparent;
    color: var(--term-text-dim);
    border-radius: 6px;
    cursor: pointer;
  }

  .mobile-menu-btn:hover {
    background: var(--term-bg);
    color: var(--term-text);
  }

  .menu-icon {
    width: 20px;
    height: 20px;
  }

  .mobile-logo {
    display: flex;
    align-items: center;
    gap: 4px;
    font-family: var(--font-mono);
    font-size: 14px;
  }

  .logo-accent {
    color: var(--term-accent);
    font-weight: bold;
  }

  .logo-text {
    color: var(--term-text);
  }

  .status-indicator {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    margin-left: auto;
  }

  .status-indicator.connected {
    background: var(--term-success);
  }

  .status-indicator.disconnected {
    background: var(--term-error);
  }

  .mobile-panel-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    border: none;
    background: transparent;
    color: var(--term-text-dim);
    border-radius: 6px;
    cursor: pointer;
  }

  .mobile-panel-btn:hover {
    background: var(--term-bg);
    color: var(--term-text);
  }

  .panel-icon {
    width: 20px;
    height: 20px;
  }

  /* Settings panel */
  .settings-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    z-index: 100;
    border: none;
    cursor: pointer;
  }

  .settings-panel {
    position: fixed;
    z-index: 101;
  }

  .settings-panel.desktop {
    left: 48px;
    top: 0;
    bottom: 0;
  }

  .settings-panel.mobile {
    left: 0;
    top: 0;
    bottom: 0;
  }
</style>
