<script lang="ts">
  import { onMount } from 'svelte';
  import type { AdvisorySessionSummary, AdvisorySessionDetail } from '$lib/types/advisory';
  import { formatCost, formatTimestamp } from '$lib/types/advisory';
  import SessionDetail from './SessionDetail.svelte';
  import DeliberationInput from './DeliberationInput.svelte';
  import DeliberationTimeline from './DeliberationTimeline.svelte';

  // API base - use relative path for same-origin requests
  const API_BASE = '/api/advisory';

  // View modes
  type ViewMode = 'list' | 'detail' | 'deliberating' | 'new';

  let sessions = $state<AdvisorySessionSummary[]>([]);
  let selectedSession = $state<AdvisorySessionDetail | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let loadingDetail = $state(false);
  let viewMode = $state<ViewMode>('list');
  let deliberationMessage = $state('');

  async function loadSessions() {
    loading = true;
    error = null;
    try {
      const res = await fetch(`${API_BASE}/sessions?limit=50`);
      if (!res.ok) throw new Error(`Failed to load sessions: ${res.status}`);
      const data = await res.json();
      sessions = data.sessions || [];
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to load sessions';
    } finally {
      loading = false;
    }
  }

  async function selectSession(id: string) {
    loadingDetail = true;
    try {
      const res = await fetch(`${API_BASE}/sessions/${id}`);
      if (!res.ok) throw new Error(`Failed to load session: ${res.status}`);
      selectedSession = await res.json();
      viewMode = 'detail';
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to load session';
    } finally {
      loadingDetail = false;
    }
  }

  function clearSelection() {
    selectedSession = null;
    viewMode = 'list';
  }

  function showNewDeliberation() {
    viewMode = 'new';
  }

  function startDeliberation(message: string) {
    deliberationMessage = message;
    viewMode = 'deliberating';
  }

  function handleDeliberationComplete(sessionId: string) {
    // Refresh sessions and view the completed session
    loadSessions();
    selectSession(sessionId);
  }

  function handleDeliberationError(err: string) {
    error = err;
    // Stay in deliberating view to show error
  }

  function cancelDeliberation() {
    viewMode = 'list';
    deliberationMessage = '';
  }

  function getStatusColor(status: string): string {
    switch (status) {
      case 'active': return 'var(--term-success)';
      case 'deliberating': return 'var(--term-warning)';
      case 'failed': return 'var(--term-error)';
      case 'archived': return 'var(--term-text-dim)';
      default: return 'var(--term-text-dim)';
    }
  }

  function getModeIcon(mode: string): string {
    switch (mode) {
      case 'council': return 'users';
      case 'single': return 'user';
      default: return 'message';
    }
  }

  onMount(() => {
    loadSessions();
  });
</script>

<div class="advisory-tab">
  {#if viewMode === 'detail' && selectedSession}
    <!-- Session Detail View -->
    <div class="detail-header">
      <button class="back-btn" onclick={clearSelection}>
        <svg class="back-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M19 12H5M12 19l-7-7 7-7" />
        </svg>
        Back
      </button>
      <span class="session-id">{selectedSession.session.id.slice(0, 8)}</span>
    </div>
    <SessionDetail session={selectedSession} />

  {:else if viewMode === 'new'}
    <!-- New Deliberation View -->
    <div class="detail-header">
      <button class="back-btn" onclick={cancelDeliberation}>
        <svg class="back-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M19 12H5M12 19l-7-7 7-7" />
        </svg>
        Back
      </button>
      <span class="header-title">New Council Deliberation</span>
    </div>
    <DeliberationInput onSubmit={startDeliberation} />

  {:else if viewMode === 'deliberating'}
    <!-- Active Deliberation View -->
    <div class="detail-header">
      <button class="back-btn" onclick={cancelDeliberation}>
        <svg class="back-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M19 12H5M12 19l-7-7 7-7" />
        </svg>
        Cancel
      </button>
      <span class="header-title">Council Deliberation</span>
    </div>
    <DeliberationTimeline
      message={deliberationMessage}
      onComplete={handleDeliberationComplete}
      onError={handleDeliberationError}
    />

  {:else}
    <!-- Session List View -->
    <div class="list-header">
      <span class="header-title">Advisory Sessions</span>
      <div class="header-actions">
        <button class="new-btn" onclick={showNewDeliberation} title="New deliberation">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M12 5v14M5 12h14" />
          </svg>
          New
        </button>
        <button class="refresh-btn" onclick={loadSessions} disabled={loading} title="Refresh sessions">
          <svg class="refresh-icon {loading ? 'spinning' : ''}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M23 4v6h-6M1 20v-6h6" />
            <path d="M3.51 9a9 9 0 0114.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0020.49 15" />
          </svg>
        </button>
      </div>
    </div>

    {#if loading}
      <div class="loading-state">
        <div class="spinner"></div>
        <span>Loading sessions...</span>
      </div>
    {:else if error}
      <div class="error-state">
        <span class="error-icon">!</span>
        <span>{error}</span>
        <button class="retry-btn" onclick={loadSessions}>Retry</button>
      </div>
    {:else if sessions.length === 0}
      <div class="empty-state">
        <svg class="empty-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
          <path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2" />
          <circle cx="9" cy="7" r="4" />
          <path d="M23 21v-2a4 4 0 0 0-3-3.87" />
          <path d="M16 3.13a4 4 0 0 1 0 7.75" />
        </svg>
        <span class="empty-title">No sessions yet</span>
        <span class="empty-hint">Council deliberations will appear here</span>
      </div>
    {:else}
      <div class="session-list">
        {#each sessions as session (session.id)}
          <button
            class="session-item"
            onclick={() => selectSession(session.id)}
            disabled={loadingDetail}
          >
            <div class="session-row">
              <span class="session-mode" title={session.mode}>
                {#if session.mode === 'council'}
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2" />
                    <circle cx="9" cy="7" r="4" />
                    <path d="M23 21v-2a4 4 0 0 0-3-3.87M16 3.13a4 4 0 0 1 0 7.75" />
                  </svg>
                {:else}
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                    <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2" />
                    <circle cx="12" cy="7" r="4" />
                  </svg>
                {/if}
              </span>
              <span class="session-topic" title={session.topic || 'Untitled'}>
                {session.topic || 'Untitled session'}
              </span>
              <span class="session-status" style="color: {getStatusColor(session.status)}">
                {session.status}
              </span>
            </div>
            <div class="session-meta">
              <span class="session-turns">{session.total_turns} turns</span>
              <span class="session-time">{formatTimestamp(session.created_at)}</span>
            </div>
          </button>
        {/each}
      </div>
    {/if}
  {/if}
</div>

<style>
  .advisory-tab {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  .list-header, .detail-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    border-bottom: 1px solid var(--term-border);
    background: var(--term-bg);
  }

  .header-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--term-text);
  }

  .header-actions {
    display: flex;
    gap: 8px;
  }

  .refresh-btn, .back-btn, .new-btn {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 8px;
    font-size: 12px;
    color: var(--term-text-dim);
    background: transparent;
    border: none;
    border-radius: 4px;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .refresh-btn:hover, .back-btn:hover, .new-btn:hover {
    color: var(--term-accent);
    background: var(--term-bg-secondary);
  }

  .new-btn {
    color: var(--term-accent);
    border: 1px solid var(--term-accent);
  }

  .new-btn:hover {
    background: rgba(var(--term-accent-rgb, 100, 149, 237), 0.15);
  }

  .new-btn svg {
    width: 14px;
    height: 14px;
  }

  .refresh-icon, .back-icon {
    width: 14px;
    height: 14px;
  }

  .refresh-icon.spinning {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  .session-id {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--term-text-dim);
  }

  /* States */
  .loading-state, .error-state, .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 48px 24px;
    color: var(--term-text-dim);
  }

  .spinner {
    width: 24px;
    height: 24px;
    border: 2px solid var(--term-border);
    border-top-color: var(--term-accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  .error-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    background: var(--term-error);
    color: white;
    border-radius: 50%;
    font-weight: bold;
  }

  .retry-btn {
    padding: 6px 12px;
    font-size: 12px;
    color: var(--term-accent);
    background: transparent;
    border: 1px solid var(--term-accent);
    border-radius: 4px;
    cursor: pointer;
  }

  .empty-icon {
    width: 48px;
    height: 48px;
    opacity: 0.5;
  }

  .empty-title {
    font-size: 14px;
    font-weight: 500;
  }

  .empty-hint {
    font-size: 12px;
    opacity: 0.7;
  }

  /* Session List */
  .session-list {
    flex: 1;
    overflow-y: auto;
    padding: 8px;
  }

  .session-item {
    display: flex;
    flex-direction: column;
    gap: 4px;
    width: 100%;
    padding: 10px 12px;
    margin-bottom: 4px;
    background: var(--term-bg);
    border: 1px solid var(--term-border);
    border-radius: 6px;
    cursor: pointer;
    text-align: left;
    transition: all 0.15s ease;
  }

  .session-item:hover {
    border-color: var(--term-accent);
    background: var(--term-bg-secondary);
  }

  .session-item:disabled {
    opacity: 0.6;
    cursor: wait;
  }

  .session-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .session-mode {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    color: var(--term-accent);
  }

  .session-mode svg {
    width: 14px;
    height: 14px;
  }

  .session-topic {
    flex: 1;
    font-size: 13px;
    font-weight: 500;
    color: var(--term-text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .session-status {
    font-size: 11px;
    font-weight: 500;
    text-transform: uppercase;
  }

  .session-meta {
    display: flex;
    justify-content: space-between;
    font-size: 11px;
    color: var(--term-text-dim);
    padding-left: 28px;
  }

  .session-time {
    font-size: 10px;
    color: var(--term-text-dim);
  }
</style>
